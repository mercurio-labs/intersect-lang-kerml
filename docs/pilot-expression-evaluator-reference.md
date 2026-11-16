# Pilot expression evaluator: behaviour reference

Status: **reference**, written 2026-11-08 as input to the
[expression evaluation plan](kerml-expression-evaluation-plan.md). It records
what the SysML v2 Pilot Implementation's evaluators do, read from the Java
source at the pinned commit `692170b71867353b8f90341e61556f49a5beb0e5`
(release 2026-08). It describes behaviour; it is not Intersect's
specification, and where the plan deviates on purpose it says so. The local
oracle jar (`jupyter-sysml-kernel-0.62.0-all.jar`, built 2026-09-11) contains
the same classes; its bytecode has not been compared with that commit, so the
oracle run, not this text, is the authority when they differ.

Abbreviations (paths in the Pilot repository):
- `L/` = `org.omg.sysml.logic/src/main/java/org/omg/sysml/`
- `X/` = `org.omg.sysml.execution/src/org/omg/sysml/execution/expressions/`

---

## 1. Architecture

### 1.1 Two evaluators

| | Model-level (`org.omg.sysml.expressions`) | Extended / "execution" (`org.omg.sysml.execution.expressions`) |
|---|---|---|
| Class | `L/expressions/ModelLevelExpressionEvaluator.java` (`INSTANCE`), implements `L/expressions/ExpressionEvaluator.java` (interface) | `X/ExpressionEvaluator.java` (`INSTANCE`) `extends ModelLevelExpressionEvaluator` |
| Function table | `L/expressions/ModelLevelLibraryFunctionFactory.java` (`INSTANCE`) | `X/LibraryFunctionFactory.java` (extends the model-level factory, adds about 40 functions) |
| Used by | the EMF operations `Expression::evaluate`, `checkCondition`, `MetadataFeature::evaluateFeature`, `MultiplicityRange::valueOf`, package/view filters, semantic-metadata base types, and `EvaluationUtil.evaluate` | `SysMLInteractive.eval` (the `%eval` command and the Jupyter `%eval` magic), PlantUML visualization (`SysML2PlantUMLText.evalInternal`) |
| Invocation of a non-library function | returns the invocation itself, unevaluated | inlines the function's result expression with arguments bound (supports user `calc def`, constraint invocation, recursion, and library functions that have bodies) |
| Unbound input parameter | evaluates to `[parameterFeature]`, unevaluated | evaluates to `[]` (treated as null) |

`ModelLevelLibraryFunctionFactory.isModelLevelEvaluable(Function)` always consults the model-level
`INSTANCE`. The extended functions therefore never make an expression "model-level evaluable".

### 1.2 Interface (`L/expressions/ExpressionEvaluator.java`)

```java
EList<Element> evaluate(Expression expression, Element target);
EList<Element> evaluateNull / evaluateLiteral / evaluateFeatureReference / evaluateMetadataAccess /
               evaluateInvocation / evaluateConstructor (X expression, Element target);
EList<Element> evaluateExpression(Expression expression, Element target, Element... arguments);
EList<Element> evaluateFeature(Feature feature, Type type);
EList<Element> evaluateFeatureChain(List<Feature> chainingFeatures, Type type);
```
Default helper methods (used by every function):
- `evaluateArgument(inv, i, target)`: returns `[]` if `i >= inv.getArgument().size()`; otherwise `evaluate(arguments.get(i), target)`.
- `argumentValue(inv, i, target)`: the single element of `evaluateArgument`. Returns Java `null` if the result is null **or its size is not 1**.
- `expressionValue(inv, i, target)`: if `argumentValue` is an `Expression`, returns `evaluate(thatExpression, target)`; otherwise `null`. Lazy operands use this (see below).
- `booleanExpressionValue`: `expressionValue` must be a single `LiteralBoolean`; otherwise `null`.
- `booleanValue` / `stringValue` / `integerValue` / `realValue`: `argumentValue` must be the matching `LiteralBoolean` / `LiteralString` / `LiteralInteger` / `LiteralRational`; otherwise `null`.

### 1.3 Value representation

- A result is an `EList<Element>`, an ordered sequence. **Java `null` (no list) means "could not evaluate"**, which is distinct from `[]` (the empty sequence, i.e. null in KerML).
- Primitive values are **newly created, unowned** literal elements made by `L/util/EvaluationUtil.java`: `literalBoolean(boolean)`, `literalString(String)`, `literalInteger(int)` (**Java `int`, 32-bit, wraps on overflow**), `literalRational(double)` (**Java `double`**; there is no BigDecimal and no true rational), and `literalInfinity()`. A literal in the source evaluates to **itself** (`evaluateLiteral` returns `singletonList(expression)`), not a copy.
- `EvaluationUtil.valueOf(Element)` maps elements to Java values: `LiteralBoolean` to `Boolean`, `LiteralString` to `String`, `LiteralInteger` to `Integer`, `LiteralRational` to `Double`, **`LiteralInfinity` to `null`**, and any other element to the element itself.
- Non-primitive results are model elements: features (for example the unevaluated feature, an enum value `E::e`, a constructor result parameter, a metadata feature, or a metaclass feature).
- **"Unevaluated"** is represented in-band. The result is a singleton list containing the expression or feature that could not be reduced: `EvaluationUtil.singletonList(invocation)`, `singletonList(feature)`, or `singletonList(resultExpression)`. The Interactive tests rely on this; for example, `p1.a2` where `a2 = a1 + 3` and `a1` is unbound yields `OperatorExpression +`.
- `singletonList(null)` returns Java `null`.

### 1.4 Dispatch on metaclass (`ModelLevelExpressionEvaluator.evaluate`)

The checks run in this order:

1. `NullExpression`: `[]` (`EvaluationUtil.nullList()`).
2. `LiteralExpression` (Boolean, String, Integer, Rational, Infinity): `[self]`.
3. `FeatureReferenceExpression`: `referent == null ? null : evaluateFeature(referent, target instanceof Type ? (Type)target : null)`.
4. `MetadataAccessExpression` (`X.metadata`): all `MetadataFeature`s of the referenced element (owned annotations whose annotating element is a `MetadataFeature`, plus owned-member `MetadataFeature`s that annotate it; `ElementUtil.getAllMetadataFeaturesOf`), followed **last** by the element's *metaclass feature* (`ElementUtil.getMetaclassFeatureFor`).
5. `InvocationExpression`, which covers all subclasses: `OperatorExpression`, `FeatureChainExpression` (operator `"."`), `IndexExpression` (`"#"`), `SelectExpression` (`"select"`), `CollectExpression` (`"collect"`), and `TriggerInvocationExpression`. The function is looked up in the factory. If found, it is invoked; otherwise the result is `[expression]` (unevaluated). The extended evaluator replaces the "otherwise" branch, see 1.7.
6. `ConstructorExpression` (`new T(...)`): `[resultParameter]` (the owned result feature, which carries the argument bindings as redefining features), or `null` if there is no result parameter.
7. Anything else (plain `Expression`, calc/constraint usages, body expressions): `evaluateExpression(expression, target)` with no arguments.

`InstantiationExpression` is abstract. Its concrete subclasses are `InvocationExpression` and `ConstructorExpression`.

### 1.5 Function lookup

```java
// ModelLevelLibraryFunctionFactory
public LibraryFunction getLibraryFunction(Function function) {
    return function == null? null: getFunctionMap().get(function.getQualifiedName());
}
// LibraryFunction default: qualified name = getPackageName() + "::" + name for each of getFunctionNames()
```
- `InvocationExpression.getFunction()` is `FeatureUtil.getFirstTypeOf(expr, Function.class)`, i.e. the first `Function` among the expression's types (`L/delegate/setting/Expression_function_SettingDelegate.java`).
- An operator expression gets an implicit `FeatureTyping` to the first library type that exists among `BaseFunctions::'op'`, `DataFunctions::'op'`, and `ControlFunctions::'op'` (`ExpressionUtil.getOperatorQualifiedNames`, `OperatorExpressionAdapter.computeImplicitGeneralTypes`). Examples: `==` maps to `BaseFunctions::'=='`, `+` to `DataFunctions::'+'`, `not` to `DataFunctions::not`, `if` to `ControlFunctions::if`, `.` to `ControlFunctions::'.'`, and `#` to `BaseFunctions::'#'`.
- Explicit invocations (`ControlFunctions::collect(x, {...})`, `(1,2,3)->ControlFunctions::select {...}`, `TrigFunctions::sin(2)`) are typed by the named function.
- The key is the **escaped qualified name** (`Element_qualifiedName_SettingDelegate`, `ElementUtil.escapeName`). A name is quoted unless it matches `[a-zA-Z_]\w*`, so the keys are `DataFunctions::not`, `ControlFunctions::if`, `BaseFunctions::istype`, `BaseFunctions::'=='`, and so on.
- **The match is by exact qualified name only.** Specialized library functions are not recognized, for example `ScalarFunctions::'+'`, `IntegerFunctions::'+'`, `RealFunctions::'*'`, `BooleanFunctions::'not'`, or `DataFunctions::'=='`. Only `BaseFunctions::'=='` is mapped. An explicit invocation of such a function is unevaluated in model-level mode, and in extended mode it falls back to inlining the function body, if one exists.
- Library *features* (constants) are looked up the same way by `Feature.getQualifiedName()` (`getLibraryFeature`). The only one is `TrigFunctions::pi`.

### 1.6 Feature evaluation, the "target", and feature chains

`target` is the context `Element`. It is used as a `Type` only if it is one; a `Package` target, for example, gives `type = null`. A `Feature` target that has feature chaining represents a **nested context path**. `EvaluationUtil.getTargetFeatureFor(target)` returns the target if it is a `Feature`; otherwise it creates a fresh `Feature` typed by the target, or an untyped one.

`evaluateFeature(feature, type)` in `ModelLevelExpressionEvaluator.java` is the core of the evaluator. Its steps, in order:

1. If the feature is a library feature (`TrigFunctions::pi`), return its value.
2. If `type != null` and the feature specializes `Base::Anything::self`, return `[getTargetFeatureFor(type)]`. This means `self` evaluates to the context. The check comes before the chain test because `self` has chaining features.
3. If the feature has owned feature chaining (`a.b.c` as a single feature), return `evaluateFeatureChain(feature.getChainingFeature(), type)`.
4. Otherwise build the list of context types: the type's chaining features if it is a chained feature, else `[type]`. Iterate it **from last to first**:
   - If `t` is a `MetadataFeature` and the feature specializes `Metaobject::annotatedElement`, return `results(t.getAnnotatedElement())`.
   - Else, if `t` is a *metaclass feature* (a `MetadataFeature` that is the `getMetaclassFeatureFor` of its annotated element) and the feature is not an `Expression`: **reflective EMF access**. Take `element.eClass().getEStructuralFeature(feature.getDeclaredName())`; if it exists, return `results(element.eGet(eFeature, true))`.
   - Else, take `typeFeature = getTypeFeatureFor(feature, t)`: the first feature in `t.getFeature()` (including inherited features) whose `FeatureUtil.getAllRedefinedFeaturesOf` contains the feature. If `typeFeature != null`: if it has a `FeatureValue` and `evaluate(valueExpr, type)` is non-null, return that. **Otherwise return `[typeFeature]` (unevaluated) immediately**, without trying outer contexts or the feature's own value.
5. After the loop, if the feature's own `FeatureValue` expression evaluates to a non-null result (evaluated with the **original** `type`), return it.
6. Otherwise return `[feature]` (unevaluated).

Notes:
- `FeatureUtil.getValueExpressionFor` takes the first owned `FeatureValue` and **does not distinguish `=` from `default` or `:=`**. Test EvalTest3: `attribute :>> a1 default 11;` evaluates to 11.
- `EvaluationUtil.results(Object)` converts reflective values. `Boolean`, `String`, `Integer`, and `Double` become new literals. An `Element` becomes **its metaclass feature**, so reflective navigation can continue. Lists are mapped with nulls dropped. A single null gives `[]`. Quirk: a single non-convertible value, such as an EMF enum, adds a Java `null` element, because the null check is applied to `object` rather than to `element`.

`evaluateFeatureChain(chain, type)`:
- Evaluate `chain[0]` in `type`. If the chain has one element, return that.
- Otherwise, for each value: if it is not a `Type`, add `chainFeatures(value, chainFeatures(rest))`. If it is a `Feature`, recurse with `target = chainFeatures(getTargetFeatureFor(type), value)`, which pushes it onto the context path. If it is a non-feature `Type`, recurse with `target = value`. Results are concatenated.
- Literals are `Feature`s, so a literal value becomes a context too.

`evaluateExpression(expr, target, args...)` is used for body expressions in collect/select/reduce and for "other" expressions:
- `invocation = EvaluationUtil.createInvocationOf(expr, args)`. This creates an `InvocationExpression` that specializes `expr`. For each Feature argument it adds an owned `in` feature that redefines the i-th parameter of `TypeUtil.getAllParametersOf(expr)`, with a `FeatureValue` that is the argument itself if it is an `Expression`, otherwise a `FeatureReferenceExpression` to it. **Non-Feature arguments are silently not bound.**
- If `expr.getFunction()` is a library function, invoke it on that invocation. This is how `->reduce DataFunctions::'*'` works.
- Otherwise evaluate the result expression of `expr` (`ResultExpressionMembership`, else the value of its result parameter) with `target = chainFeatures(getTargetFeatureFor(target), invocation)`. A null result becomes `[resultExpression]`. With no result expression the result is `[expr]`.

### 1.7 Extended evaluator additions (`X/ExpressionEvaluator.java`)

`evaluateInvocation`, when the function is not a library function:
```java
Type type = expression.instantiatedType();
Expression resultExpression = EvaluationUtil.getResultExpressionFor(type);
if (resultExpression == null) return singletonList(expression);
Feature targetFeature = getTargetFeatureFor(target);
if (type instanceof Feature) targetFeature = chainFeatures(targetFeature, (Feature)type);
results = evaluate(resultExpression, chainFeatures(targetFeature, instantiateInvocation(expression, targetFeature)));
return results == null ? singletonList(resultExpression) : results;
```
- `instantiateInvocation` creates a new `InvocationExpression` typed by the same instantiated type. For each owned parameter of the original invocation that has a value expression, it adds a new parameter redefining the same features. That parameter's value is `EvaluationUtil.expressionFor(evaluate(valueExpr, target), expression)`: the argument is **evaluated eagerly in the caller's context**, then re-encoded as an expression (a literal, a `FeatureReferenceExpression`, a `NullExpression` for `[]`, or a `','` chain for lists). If the values cannot be encoded (for example an unevaluated `InvocationExpression`), the parameter is left without a value.
- This gives positional and named arguments, constraint invocation (`p1.c11()`), calc usages invoked through a feature (`p1.b(1,2,3)`), and recursion (the `Fact` test).
- Library functions that are not in the map but have a body in the library are inlined the same way. For example, `CollectionFunctions::size`, `contains`, `'=='`, and `'#'`, and `ControlFunctions::allTrue` and `anyTrue`, all pass in `ExpressionEvaluationTest`.

`evaluateFeature` override: if the super result is a single `Feature` that is not an `Expression`, and that feature is an input parameter (`in` or `inout`) of `type`, the method returns `[]`. That is, an unbound input parameter is treated as null (test: `Test(1)` returns `[1]` for body `(x, y)`).

---

## 2. Library functions

Sources: `L/expressions/functions/**` for model-level, and `X/functions/**` for extended-only.

Common dispatch for arithmetic and relational functions (`data/ArithmeticFunction.java`):
```java
Object x = valueOf(argumentValue(inv, 0, target));   // argumentValue: null unless exactly 1 value
Object y = valueOf(argumentValue(inv, 1, target));
return numberOfArgs(inv) == 1 ?
         x instanceof Integer ? unaryIntegerOp((Integer)x) :
         x instanceof Double  ? unaryRealOp((Double)x) :
         nullList() :                                          // unary on non-number -> []  (!)
       x Integer && y Integer ? binaryIntegerOp(x, y) :
       x Double  && y Integer ? binaryRealOp(x, (double)y) :
       x Integer && y Double  ? binaryRealOp((double)x, y) :
       x Double  && y Double  ? binaryRealOp(x, y) :
       x String  && y String  ? binaryStringOp(x, y) :
       singletonList(inv);                                     // unevaluated
// Default (un-overridden) ops return Java null, so the whole invocation returns null.
```
Consequences:
- Operands must be **exactly one** evaluated value. `null`, `()`, `*` (infinity has a null value), a list of two or more, a Boolean, an element, or an unevaluated feature all leave the expression unevaluated (`[inv]`).
- **Unary `+` or `-` on a non-number returns `[]`, not unevaluated.** Examples: `-x` with `x` unbound, `-null`, `-"a"`, `-true`.
- An operation the function does not define returns Java `null`. Examples: `"a" - "b"`, `"a" * "b"`, `"a" / "b"`, `"a" ** "b"`, `"a" % "b"`.
- Mixed int/real promotes to `double`. There is no Integer-to-String coercion: `"a" + 1` is unevaluated.

### 2.1 Model-level table (`ModelLevelLibraryFunctionFactory.initialize`)

| Qualified name (factory key) | Syntax | Class | Semantics |
|---|---|---|---|
| `BaseFunctions::'=='` | `==` | `base/EqualsFunction` | `x = evalArg(0)`, `y = evalArg(1)` (whole lists). If **both** are a single Feature specializing `Collections::Collection`, each is replaced by its `elements` value (`getElementsOf`, via the chain `[c, Collection::elements]`). Either list Java null gives `[inv]`. Otherwise `booleanResult(EvaluationUtil.equal(x, y))`: same size and element-wise `equal` (see 2.3). Lists of any length are compared. |
| `BaseFunctions::'!='` | `!=` | `NotEqualsFunction` | negation of the above |
| `BaseFunctions::'==='` | `===` | `SameFunction extends EqualsFunction` | **identical to `==`** (value equality; there is no identity semantics) |
| `BaseFunctions::'!=='` | `!==` | `NotSameFunction extends NotEqualsFunction` | identical to `!=` |
| `BaseFunctions::istype` | `x istype T` (also the unary `istype T` on self) | `IsTypeFunction` | `T = getTypeArgument(inv)`: the first type of the **second owned feature** of the invocation (the TypeReference). Null gives `[inv]`. Values = evalArg(0); null gives `[inv]`. Returns `allMatch(isType)`, so the **empty sequence gives true**. |
| `BaseFunctions::hastype` | `hastype` | `HasTypeFunction` | `allMatch(hasType)`: exact type membership, `getType(...).contains(T)`; empty gives true |
| `BaseFunctions::'@'` | `@` | `AtFunction` | `anyMatch(isType)`; empty gives false |
| `BaseFunctions::'@@'` | `X @@ T` (X is a `MetadataAccessExpression`) | `AtAtFunction extends AtFunction` | same as `@`, applied to the metadata list (which includes the metaclass feature) |
| `BaseFunctions::as` | `x as T` | `AsFunction` | filters values: keeps those where `isType` holds (order kept). T null or values null gives `[inv]`. |
| `BaseFunctions::meta` | `X meta T` | `MetaFunction extends AsFunction` | same as `as`, on the metadata list. `(P meta SysML::PartDefinition).isAbstract` then works through reflective access. |
| `BaseFunctions::','` | `(a, b)` | `ListConcatFunction` | `evalArg(0) ++ evalArg(1)`; either null gives `[inv]`. A nested `(a,(b,c))` flattens. |
| `BaseFunctions::'#'` | `seq#(i)`, `arr#(i,j,...)` | `IndexFunction` | **1-based.** If seq is a single Feature specializing `Collections::Array`: dims = `evaluateFeatureChain([arr, Array::dimensions], arr)`. The index count must equal the dims count, and each dim and index must be a `LiteralInteger` with `idx <= dim`; otherwise `[inv]`. Flat index uses `flat = dim*(flat-1)+idx` (row-major), then indexes `elements`. Else, if there is exactly one `LiteralInteger` index: if seq is a single Feature specializing `Collections::OrderedCollection`, index its `elements` (null gives `[inv]`); otherwise index the sequence directly. **Out of range (`<1` or `>size`) gives `[]`.** Any other index shape gives `[inv]`. There is no lower-bound check on array indexes (`idx <= 0` is not rejected). |
| `DataFunctions::'..'` | `a..b` | `data/ListRangeFunction` | both must be Integer, else `[inv]`. Returns `[a, a+1, ..., b]` as new LiteralIntegers, inclusive; `[]` if `a > b`. |
| `DataFunctions::'+'` | `+x`, `x+y` | `PlusFunction` | unary returns its operand as a new literal. int+int is `int` (wraps); otherwise `double`. **String+String concatenates.** |
| `DataFunctions::'-'` | `-x`, `x-y` | `MinusFunction` | unary negation (`-Integer.MIN_VALUE` wraps); int or double subtraction |
| `DataFunctions::'*'` | `*` | `TimesFunction` | int (wrapping) or double |
| `DataFunctions::'/'` | `/` | `DivideFunction` | **Always real.** int/int is computed as `(double)x/(double)y`, so `2/3 = 0.666...` and `4/2 = 2.0` (`LiteralRational`). **A divisor equal to `0.0` gives `[]`** (for both int and real; `-0.0 == 0.0` in Java). |
| `DataFunctions::'**'`, `DataFunctions::'^'` | `**`, `^` | `PowerFunction` | `Math.pow(x, y)` and **always real**, even for int**int (`2**3 = 8.0`). No zero or NaN guard: `0**-1 = Infinity`, `(-8)**(1/3)` is NaN. |
| `DataFunctions::'%'` | `%` | `RemainderFunction` | int: Java `x % y` (truncated; the sign follows the dividend, so `-7 % 3 = -1`). real: Java `%` (fmod). **A divisor of zero gives `[]`.** |
| `DataFunctions::'<'`, `'<='`, `'>'`, `'>='` | relational | `LessThanFunction` etc. | int/int, promoted int/real, and real/real; **String uses `String.compareTo`** (UTF-16 code unit order). Booleans and other values give `[inv]`. Unary use returns Java null. |
| `DataFunctions::not` | `not x` | `bool/NotFunction` | `x = booleanValue(0)`, which is null unless the argument is a single LiteralBoolean. Result: `booleanResult(x != TRUE)`, so **`not null`, `not 1`, and `not <unevaluated>` are all `true`**. |
| `DataFunctions::'&'` | `&` | `AndFunction` | `x==TRUE & y==TRUE`. Both operands are evaluated (not short-circuit). Non-Booleans count as false. **Never unevaluated.** |
| `DataFunctions::'\|'` | `\|` | `OrFunction` | `x==TRUE \| y==TRUE` |
| `DataFunctions::xor` | `xor` | `XorFunction` | `x==TRUE ^ y==TRUE` |
| `ControlFunctions::'.'` | `x.f` (`FeatureChainExpression`) | `control/DotFunction` | `list = evalArg(0)`; null gives `[inv]`. `targetFeature = ExpressionUtil.getTargetFeatureFor(inv)`, the member element of the **second owned membership**; if it is not a Feature, `[inv]`. For each element of the list that is a `Feature` (others are **silently skipped**), evaluate the chain `[element] ++ (target's chaining features, or [target])` with `type = target instanceof Type ? target : null`. Non-null results are concatenated. |
| `ControlFunctions::if` | `if c ? a else b` | `ConditionalFunction` | `test = booleanValue(0)`: null (not exactly one Boolean) gives `[inv]`. `true` gives `expressionValue(1)`, `false` gives `expressionValue(2)`. **Lazy**: in operator syntax, operands 2 and 3 are `FeatureReferenceExpression`s to owned expressions. The reference evaluates to `[thatExpression]`, which is then evaluated. A branch that is not a single Expression gives Java null. |
| `ControlFunctions::and` | `and` | `ConditionalAndFunction` (`ConditionalLogicalFunction`) | `first = booleanValue(0)`. If `first != TRUE`, return **false** (so non-Boolean or unevaluated also gives false). Else `second = booleanExpressionValue(1)` (lazy); null gives `[inv]`, otherwise the second value. |
| `ControlFunctions::or` | `or` | `ConditionalOrFunction` | `first == TRUE` gives **true**; otherwise the lazy second operand (non-Boolean gives `[inv]`) |
| `ControlFunctions::implies` | `implies` | `ConditionalImpliesFunction` | `first != TRUE` gives **true**; otherwise the lazy second operand |
| `ControlFunctions::'??'` | `a ?? b` | `NullCoalescingFunction` | `list = evalArg(0)`: null gives `[inv]`; `[]` gives `expressionValue(1)` (lazy); otherwise `list`. An unevaluated left side (`[feature]`) is non-empty, so it is returned. `null ?? false` gives `false`. |
| `ControlFunctions::collect` | `seq.{in x; ...}`, `->collect` | `CollectFunction` (`ControlFunction.collectSelected`) | `list = evalArg(0)`; `expr = argumentValue(1)` must be an `Expression` (the body reference evaluates to the body expression), else `[inv]`. For each value: a Java-null element gives `[inv]`. Otherwise `r = evaluateExpression(expr, target, value)`, and non-null results are concatenated (**a null `r` is silently dropped**). |
| `ControlFunctions::select` | `seq.?{in x; ...}`, `->select` | `SelectFunction` | same loop; keeps `value` iff `r` is a single element with `valueOf == Boolean.TRUE` |
| `TrigFunctions::pi` (feature) | `TrigFunctions::pi` | `trig/PiFeature` | `realResult(Math.PI)` |

Unknown functions: any other function, including `BaseFunctions::'['` (quantity `5 [kg]`), `BaseFunctions::all`, `DataFunctions::'~'`, `DataFunctions::max`/`min`, `BaseFunctions::ToString`, all `RealFunctions`/`IntegerFunctions`/`StringFunctions`/`SequenceFunctions`/`CollectionFunctions`, and user functions, evaluates to `[invocation]` in model-level mode.

### 2.2 Extended-only functions (`X/LibraryFunctionFactory.java`)

| Qualified name | Class | Semantics |
|---|---|---|
| `ControlFunctions::forAll` | `control/ForAllFunction` | Each value is evaluated through `evaluateExpression(body, target, v)`. Any result that is null or not of size 1 makes the whole function `[inv]`. Result is false at the first non-true value; empty gives true. |
| `ControlFunctions::exists` | `ExistsFunction` | `!forAll(test=false)`: an element whose body is not `false` makes it true. Empty gives false. |
| `ControlFunctions::reject` | `RejectFunction` | keeps a value iff its body result is a single `FALSE` |
| `ControlFunctions::selectOne` | `SelectOneFunction` | first element of `select`, or `[]` |
| `ControlFunctions::reduce` | `ReduceFunction` | empty gives `[]`; otherwise a left fold `evaluateExpression(f, target, acc, v)`. A null or non-size-1 step gives `[inv]`. |
| `ControlFunctions::maximize` / `minimize` | `MaximizeFunction` / `MinimizeFunction` | map through the body, then fold with `DataFunctions::max` / `min` (invoked through a created invocation); empty gives `[]` |
| `DataFunctions::max` / `min` | `data/MaxFunction` / `MinFunction` (ArithmeticFunction) | int, promoted real, or String (`compareTo`) |
| `NumericalFunctions::sum` / `product` | `numerical/SumFunction` / `ProdFunction` | Over a list of LiteralInteger or LiteralRational. Integer accumulation stays `int` until the first Rational, then switches to `double`. Any other element gives `[inv]`. Empty gives `0` / `1` (Integer). |
| `SequenceFunctions::size`, `isEmpty`, `notEmpty` | `sequence/*` | on the evaluated list (`null` gives `[inv]`) |
| `SequenceFunctions::includes`, `includesOnly`, `excludes` | | Compare with `EvaluationUtil.equal`. `includes(s, [])` is true. `includesOnly` is mutual inclusion (a set comparison, ignoring order and multiplicity). |
| `SequenceFunctions::equals`, `same` | `SequenceEqualsFunction`, `SequenceSameFunction` | both `EvaluationUtil.equal(list1, list2)`, i.e. ordered, element-wise |
| `SequenceFunctions::including` = `union` | `IncludingFunction extends UnionFunction extends ListConcatFunction` | concatenation |
| `SequenceFunctions::includingAt(seq, values, index)` | | Inserts before 1-based `index`; `*` means the end; a negative index is clamped to 0. |
| `SequenceFunctions::excluding`, `intersection` | | filter by `equal` |
| `SequenceFunctions::excludingAt(seq, start, end?)` | | removes positions start..end (1-based, inclusive; `*` means to the end). Has an operator-precedence bug in its type check: `!(s==null \|\| s instanceof Integer && e==null \|\| e instanceof Integer)`. |
| `SequenceFunctions::subsequence(seq, start, end?)` | | 1-based inclusive; `*` start gives `[]`; `end < start` gives `[]` |
| `SequenceFunctions::head`, `last`, `tail` | | empty gives `[]` |
| `StringFunctions::Length` | `string/StringLengthFunction` | `String.length()` (UTF-16 units) |
| `StringFunctions::Substring(s, lower, upper)` | `StringSubstringFunction` | 1-based inclusive; `lower<1 \|\| upper>len \|\| lower>upper+1` gives `[]` |
| `TrigFunctions::sin`, `cos`, `tan`, `cot` (`1/Math.tan`), `arcsin`, `arccos`, `arctan`, `deg` (`Math.toDegrees`), `rad` (`Math.toRadians`) | `trig/*` (`TrigFunction`) | Integer or Real argument (Integer is converted to double); anything else gives `[inv]`. The result is always `LiteralRational`. |

### 2.3 Equality (`EvaluationUtil.equal`)

```java
public static boolean equal(Element x, Element y) {
    Object x_value = valueOf(x); Object y_value = valueOf(y);
    return x_value == null? y_value == null: x_value.equals(y_value);
}
```
- Java boxed `equals` is used, so **`1 == 1.0` is `false`** (`Integer.equals(Double)` is false) and `2.0 == 2.0` is true. `Double.equals` treats NaN as equal to NaN and `0.0` as different from `-0.0`.
- `*` (LiteralInfinity) has value null, so `* == *` is true.
- `null == null` compares `[]` with `[]` and gives **true**. `null == 1` gives **false** (sizes differ).
- Non-literal elements are compared by **object identity**, including two distinct unevaluated features. **An unevaluated operand does not make `==` unevaluated.** `x == 1` with `x` unbound gives **false**, because `[x]` is compared with `[1]`. `==` is unevaluated only when an operand evaluation returns Java null.
- Literal versus literal compares by value, regardless of whether the literal is new or from the source.

### 2.4 Type tests (`EvaluationUtil.getType` / `isType` / `hasType`)

- A `LiteralExpression` has exactly one type: `ScalarValues::Boolean`, `String`, `Integer`, **`Rational`** (for `LiteralRational`, not `Real`), or **`Positive`** (for `LiteralInfinity`). `getPrimitiveType` resolves it by eClass.
- A `Feature` has the types in `feature.getType()`. A metaclass feature is typed by its metaclass, so `@SysML::PartUsage` and `hastype SysML::PartUsage` work.
- Any other element has no types.
- `isType` is `TypeUtil.specializes(elementType, T)` for any of the element's types. Through ScalarValues specialization, `1 istype Real` is true and `1 istype Natural` is **false**, because the literal type is `Integer`, not `Natural`.
- `hasType` is exact membership.

---

## 3. isModelLevelEvaluable

`Expression.isModelLevelEvaluable` (`L/delegate/setting/Expression_isModelLevelEvaluable_SettingDelegate.java`) is `modelLevelEvaluable(new BasicEList<>())`. The `modelLevelEvaluable(visited)` operation is defined per metaclass in `L/delegate/invocation/*_modelLevelEvaluable_InvocationDelegate.java`:

| Metaclass | Rule |
|---|---|
| `Expression` (default) | Every parameter (`TypeUtil.getAllParametersOf`) must have direction `in`, or be the result parameter, **and** have no owned features **and** no `FeatureValue`. Also, the result expression (if any) must be model-level evaluable. |
| `LiteralExpression`, `NullExpression`, `MetadataAccessExpression` | `true` |
| `InvocationExpression` (all operator, chain, index, select, and collect forms) | `function != null && function.isModelLevelEvaluable() && all getArgument() are modelLevelEvaluable(visited)` |
| `ConstructorExpression` | all arguments model-level evaluable |
| `FeatureReferenceExpression` | Referent null, or a specialization of `Base::Anything::self`, gives true. A referent already in `visited` gives false (cycle). Otherwise, with the referent added to `visited`: true if the referent is an `Expression` that is itself model-level evaluable; else true if the referent's **owning type is a `Metaclass` or a `MetadataFeature`**; else **false if the referent has any featuring type**; else true if its value expression is null or model-level evaluable. |
| `CalculationUsage`, `ConstraintUsage` | `false` |

`Function.isModelLevelEvaluable` (`Function_isModelLevelEvaluable_SettingDelegate`; reused by `CalculationDefinition` and `ConstraintDefinition`) is `ModelLevelLibraryFunctionFactory.isModelLevelEvaluable(function)`, i.e. the function is a key in the **model-level** map of section 2.1.

Consequences, confirmed by `org.omg.sysml.interactive.tests/.../ModelLevelEvaluationTest.java`:
- Literals, arithmetic, comparison, Boolean operators, `if`, `??`, `null`, `()`, lists, and `..` are evaluable.
- `null.{in x; 1}` and `ControlFunctions::collect(null, {in x; 1})` are evaluable.
- `SequenceFunctions::size(...)`, `StringFunctions::Length(...)`, `NumericalFunctions::sum(...)`, and `f(3+4)` for a user `calc def f` are **not** evaluable.
- A body that references its own parameter, as in `(1,2,3).{in x; x*2}`, is **not** model-level evaluable, because parameters have featuring types. The test file says this explicitly. The model-level *evaluator* would still compute it if called directly.

Gating: the EMF operation `Expression::evaluate(target)` gates **only at `InvocationExpression`**.
- `InvocationExpression_evaluate_InvocationDelegate`: `self.isModelLevelEvaluable() ? ModelLevelExpressionEvaluator.INSTANCE.evaluateInvocation(self, target) : null`.
- `LiteralExpression`, `FeatureReferenceExpression`, and `MetadataAccessExpression` delegate to the evaluator **without gating**. `NullExpression` returns `[]`.
- The default `Expression_evaluate_InvocationDelegate` evaluates `ExpressionUtil.getResultExpressionOf(self)` (its `ResultExpressionMembership`), or returns `null` if there is none. There is no `ConstructorExpression` evaluate delegate, so `ctorExpr.evaluate()` returns null at top level. The evaluator itself handles constructors when they are reached internally.
- `CalculationUsage.evaluate` and `ConstraintUsage.evaluate` always return `[]`.
- Inside the evaluator, nested sub-expressions are dispatched directly (`ModelLevelExpressionEvaluator.evaluate`), with no further gating.

`checkCondition(target)`: the `Expression`, `CalculationUsage`, and `ConstraintUsage` delegates call `ExpressionUtil.checkConditionOn(target, self)`:
```java
if (condition == null) return true;
EList<Element> result = condition.evaluate(element);
return result != null && result.size() == 1 && result.get(0) instanceof LiteralBoolean && ((LiteralBoolean)result.get(0)).isValue();
```
`ExpressionUtil.checkConditionsOn(element, conditions)` is used by filters:
- `element == null` gives false.
- The candidates are all metadata features of the element plus its metaclass feature. **Every** condition must hold. If there are no candidates, a condition is checked with `checkCondition(null)`; otherwise it must hold for **any** candidate.
- So a filter is evaluated with each `MetadataFeature` as `target`. `@Safety` is a self-reference `'@'` test, and `Safety::isMandatory` resolves through the metadata feature's redefinition.

`MetadataFeature::evaluateFeature(baseFeature)` (also on `MetadataUsage`) is `ModelLevelExpressionEvaluator.INSTANCE.evaluateFeature(baseFeature, self)`. No Java or Xtend caller was found in the repo; it is API only.

---

## 4. Callers

| Caller | Evaluator / entry | Purpose | Failure handling |
|---|---|---|---|
| `L/delegate/invocation/Package_includeAsMember_InvocationDelegate.java` used by `L/adapter/PackageAdapter.getImportedMembership` and by Xtext scoping `org.omg.kerml.xtext/.../scoping/KerMLScope.xtend` (`isIncludeAsMember`, `visitQualifiedName`) | `checkConditionsOn(element, pkg.getFilterCondition())` | Package `filter` conditions and filtered imports (`import P::*[@Safety];`, which creates a package with `ElementFilterMembership`s). Removes imported memberships and names that fail. | **Silent**: an element that is not exactly `[true]` is excluded |
| `L/delegate/invocation/ViewUsage_includeAsExposed_InvocationDelegate.java`, `L/delegate/setting/ViewUsage_exposedElement_SettingDelegate.java` | `checkConditionsOn(element, UsageUtil.getAllViewConditionsOf(view))` | SysML view `filter` and exposed elements | silent exclusion |
| `L/delegate/invocation/MultiplicityRange_valueOf_InvocationDelegate.java` | `bound.isModelLevelEvaluable()` then `bound.evaluate(mult.getOwningNamespace())` | Multiplicity bound value: `*` gives -1; an int >= 0 gives that value; anything else gives **-2** ("null"). Used by `MultiplicityRange_hasBounds` (for example `ControlNode_multiplicityHasBounds`) and by the validator. | Silent in the delegate; **reported** by the validator (next row) |
| `org.omg.kerml.xtext/src/org/omg/kerml/xtext/validation/KerMLValidator.xtend` `checkMultiplicityRange` (line about 1337) | `if (b.isModelLevelEvaluable) mult.valueOf(b) == -2 else !b.isInteger` | `validateMultiplicityRangeBoundResultTypes` | **Error** "Must have a Natural value". Negative, real, or unevaluable bounds are errors. A non-evaluable bound is checked statically with `isInteger`, which is true for an int literal, `*`, a result typed `ScalarValues::Integer`, or an operator `- + * % ^ **` with Integer operands. |
| `KerMLValidator.checkMetadataBodyFeature` (line about 1400) | `value.isModelLevelEvaluable` | Metadata body feature values must be model-level evaluable | **Error** "Must be model-level evaluable" (`invalidateMetadataFeatureBody`). Not evaluated. |
| `KerMLValidator.checkElementFilterMembership` (line about 1413) | `condition.isModelLevelEvaluable`, then static `isBoolean` | `validateElementFilterMembershipIsModelLevelEvaluable` / `...IsBoolean` | **Error** "Must be model-level evaluable" / "Must have a Boolean result". Static checks only. |
| `L/adapter/TypeAdapter.getBaseTypes` (line 495) | `expr.evaluate(metadataFeature)` for features specializing the semantic-metadata `baseType` | Semantic metadata (`#keyword` user-defined keywords) adds implicit supertypes; takes `results.get(0)` and its `getMetaclassReferenceOf` | **Silent**: null or empty results are skipped |
| `org.omg.kerml.xtext.ui/.../outline/KerMLOutlineTreeProvider.xtend` line 234 | `isModelLevelEvaluable` | Appends " model-level" to outline labels | UI only |
| `org.omg.sysml.interactive/src/org/omg/sysml/interactive/SysMLInteractive.java` `eval` (line 300) | **extended** `ExpressionEvaluator.INSTANCE.evaluate(expr, target)` | `%eval` REPL command and the Jupyter `%eval` magic (`org.omg.sysml.jupyter.kernel/.../magic/Eval.java`) | Parse or validation errors are printed as issues; an evaluation result of Java null prints `""`; unevaluated results are printed as elements |
| `org.omg.sysml.plantuml/src/org/omg/sysml/plantuml/SysML2PlantUMLText.java` `evalInternal` (line 701) and `VStructure.java` (`EvaluationUtil.valueOf`) | extended evaluator | Shows evaluated values in diagrams. `null`, `[]`, or `[self]` (unevaluated) count as no value. | silent |
| `EvaluationUtil.evaluate(expr, target)` | `ModelLevelExpressionEvaluator.INSTANCE.evaluate` | utility | n/a |

`SysMLValidator.xtend` contains no expression evaluation. No validator **evaluates** constraint, derived-value, or `assert` expressions; there is no constraint checking at validation time.

---

## 5. What is not evaluated

- **Quantities and units**: `5 [kg]` is `OperatorExpression '['`, i.e. `BaseFunctions::'['`. It is not in either map and is abstract, so in model-level mode it gives `[inv]` (and is not model-level evaluable); extended mode also leaves it unevaluated. There is no special handling of `ISQ`, `SI`, `Quantities`, `MeasurementReferences`, or `QuantityValue`. The only `ScalarValues` dependency is the literal typing in 2.4.
- **Real/Integer/Complex/Rational math functions** (`RealFunctions::sqrt`, `floor`, `round`, `abs`, `IntegerFunctions::*`, `NaturalFunctions`), `DataFunctions::'~'`, `BaseFunctions::all` (extent), and `BaseFunctions::ToString` / `ToBoolean` / `ToInteger` are not implemented in either mode. In extended mode they inline only if the library gives them a body. Most are abstract, so the result is `[inv]`.
- **String functions** are model-level: only `'+'` concatenation and the relational operators. In extended mode, `StringFunctions::Length` and `Substring` are added. Comparisons between strings and numbers are unevaluated.
- **User-defined functions, calc defs, constraint defs, calc/constraint usages**: model-level mode gives `[invocation]` (and `isModelLevelEvaluable` is false). Extended mode inlines them (1.7). `CalculationUsage.evaluate` and `ConstraintUsage.evaluate` (the EMF operation) always return `[]`.
- **`TriggerInvocationExpression`** (`at`, `after`, `when`): unevaluated.
- **Unbound features** evaluate to `[feature]`. Arithmetic on them gives `[inv]`; unary `-` gives `[]`; Boolean operators treat them as false; `==` compares by identity.
- **Overflow and precision**: Integer arithmetic is 32-bit `int` and wraps silently. Literal integers are `EInt` (the `DECIMAL_VALUE` terminal is converted to an int). Reals are IEEE `double`; there is no exact rational arithmetic, so `LiteralRational` holds a `double`.
- **Null-safety gaps**: `MetadataAccessExpression` with an unresolved element (NPE path), and `EvaluationUtil.results` adding a Java `null` element for enum-valued reflective attributes.

---

## 6. Oracle: `%eval` output format and Java API

### 6.1 `SysMLInteractive.eval(String input, String targetName)` (`SysMLInteractive.java` lines 300-337)

- No target: parse `"calc{\n" + input + "}"` in a temporary SysML resource that is not added to the index, and use `target = null`.
- With a target: `target = resolve(targetName)`, which must be fully qualified. If resolution fails, the method returns `"ERROR:Couldn't resolve reference to Element '<name>'\n"`. Otherwise it parses `"calc{private import " + targetName + "::*;\n" + input + "}"`, so names resolve within the target, and the target element is the evaluation context. The context acts as a type only if the target is a `Type`.
- Parse or validation errors: the method returns `SysMLInteractiveResult.toString()`, which lists syntax errors, or semantic errors followed by warnings, one Xtext `Issue.toString()` per line.
- Otherwise it takes the calc's `ResultExpressionMembership` expression, evaluates it with the **extended** evaluator, calls `removeResource()`, and returns `""` if the result is Java null. Otherwise it returns `elements.stream().map(SysMLInteractiveUtil::formatElement).collect(joining())`.

`SysMLInteractiveUtil.formatElement` (`SysMLInteractiveUtil.java`) produces one line per result element:
```
<EClassName>[ <<shortName>>][ <name>] (<elementId>)\n
```
- Here `name = nameOf(element)`. For literals that is the value text: `Boolean.toString`, the raw string value **without quotes or escaping**, `Integer.toString`, `Double.toString` (so `0.6666666666666666`, `4.0`, `1.0E10`, `Infinity`, `NaN`), or `*` for infinity.
- Other kinds: a chained feature prints its chaining names joined with `.`; a `FeatureReferenceExpression` prints its referent's name; an `OperatorExpression` prints its operator (`OperatorExpression +`); an `InvocationExpression` prints its function's name; anything else prints `element.getName()`.
- A metaclass feature prints `MetadataFeature` followed by ` <AnnotatedEClass>[ <<short>>][ <name>] (<id>)`.
- `elementId` is a random UUID for non-library elements, created lazily, so it differs between runs. **An oracle must strip the trailing ` (...)`.** A string containing a newline breaks line-based parsing.
- Examples from `ExpressionEvaluationTest.java`: `LiteralInteger 5 (…)`, `LiteralRational 0.6666666666666666 (…)`, `LiteralBoolean true (…)`, `LiteralString b (…)`, `AttributeUsage a1 (…)` (unevaluated feature), `OperatorExpression < (…)`. An empty sequence prints as an empty string, so `null`, `()`, and Java null are indistinguishable in `%eval` output.

How to run it:
- REPL: `java -cp jupyter-sysml-kernel-0.62.0-all.jar org.omg.sysml.interactive.SysMLInteractive <sysml.library dir> [apiBasePath]` reads stdin.
- Model text: send a line `%` followed by the model lines and a closing `%` line.
- Evaluation: `%eval [--target <QName>|--target=<QName>] <expr>`. Each prompt prints `<counter>> `.
- Jupyter magic: `%eval [--target=NAME] EXPR`.

### 6.2 Direct Java API for an oracle program

```java
SysMLInteractive si = SysMLInteractive.getInstance();   // or createInstance()
si.loadLibrary("<path>/sysml.library");                  // reads Kernel, Systems, Domain libraries
SysMLInteractiveResult r = si.process("package T { attribute a = 1 + 2; }");  // added to index
String out = si.eval("T::a * 2", null);                  // extended evaluator, formatted text
// Model-level, structured:
SysMLInteractiveResult c = si.process("calc test { 2 / 3 }", false);
Type calc = (Type)((Namespace)c.getRootElement()).getOwnedMember().get(0);
Expression e = ExpressionUtil.getResultExpressionOf(calc);   // or TypeUtil.getFeatureByMembershipIn(calc, ResultExpressionMembership.class)
boolean mle = e.isModelLevelEvaluable();
EList<Element> v1 = e.evaluate(target);                                      // EMF op, gated (invocations need isModelLevelEvaluable)
EList<Element> v2 = ModelLevelExpressionEvaluator.INSTANCE.evaluate(e, target); // model-level, ungated
List<Element> v3 = org.omg.sysml.execution.expressions.ExpressionEvaluator.INSTANCE.evaluate(e, target); // extended
// Print values: EvaluationUtil.valueOf(el) gives Boolean/String/Integer/Double/null(infinity)/Element;
// or SysMLInteractiveUtil.formatElement(el) / nameOf(el).
// Filters: pkg.includeAsMember(el); multiplicity: mult.valueOf(bound) (-1 = *, -2 = invalid).
```
The pattern in `ModelLevelEvaluationTest.java` is to wrap the expression as `"calc test { <expr> }"` (or `"package test { filter <expr>; }"` for filter conditions), then call `isModelLevelEvaluable()` and `evaluate(target)`. Metadata-context tests pass a `MetadataFeature`, or the element's metaclass feature from `ElementUtil.getMetaclassFeatureFor`, as `target`.

Distinguishing `null` from `[]` from unevaluated: use the structured API. `null` means not evaluable or gated. `[]` is the empty sequence. `[e]` where `e` is the original expression or feature (`e == expr`, or `e` is not a literal) means unevaluated.

---

## 7. Behaviours worth their own coverage-table rows

1. `2/3` gives `LiteralRational 0.666…` and `4/2` gives `2.0`. `x/0` and `x%0` give `[]`.
2. `2**3` gives `8.0` (real); `^` is the same function.
3. `1 == 1.0` gives false; `null == null` gives true; `* == *` gives true; `x == 1` with x unbound gives false.
4. `not 1` and `not null` give true. `1 & true` gives false. `x and y` with x non-Boolean gives false. `false implies <anything>` gives true. `true or <anything>` gives true; the second operand is never evaluated.
5. `-x` with x unbound gives `[]`; `x + 1` with x unbound gives `[OperatorExpression +]`; `"a" - "b"` gives Java null.
6. `5..3` gives `[]`; `(1,2,3)#(4)` gives `[]`; `#` is 1-based; arrays use row-major flat indexing over `Collections::Array::dimensions`.
7. `istype` on `[]` gives true and `@` on `[]` gives false. Literal types are Integer, Rational, Boolean, String, and Positive (for `*`).
8. Collections: `==` dereferences `elements` only when **both** sides are collections.
9. Feature lookup: the first redefinition in the most specific context wins. A redefinition found without a value gives `[thatRedefiningFeature]` and does not fall back further. `default` values count as values.
10. `===` and `!==` are identical to `==` and `!=`.
