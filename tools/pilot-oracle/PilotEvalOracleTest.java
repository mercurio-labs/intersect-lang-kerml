/* Handwritten independent verification oracle tests, never production.
 * Milestone-only: requires the pinned Pilot JVM runtime; not a quick-tier test.
 */
import java.util.List;
import java.util.Map;
import org.eclipse.emf.common.util.URI;
import org.eclipse.emf.ecore.resource.impl.ResourceImpl;
import org.eclipse.xtext.resource.XtextResourceSet;
import org.omg.sysml.lang.sysml.*;

public final class PilotEvalOracleTest {
    private static final SysMLFactory F = SysMLFactory.eINSTANCE;
    private static int checks;

    private static void check(boolean success, String message) {
        checks++;
        if (!success) throw new AssertionError(message);
    }

    private static void own(Element owner, Relationship relationship, Element child) {
        owner.getOwnedRelationship().add(relationship);
        relationship.getOwnedRelatedElement().add(child);
    }

    private static void normalization() {
        var loaded = new PilotEvalOracle.Loaded(new XtextResourceSet());
        var integer = F.createLiteralInteger();
        integer.setValue(-3);
        check(PilotEvalOracle.value(loaded, integer).equals(Map.of("kind", "int", "value", -3)), "integer");
        var real = F.createLiteralRational();
        real.setValue(Double.longBitsToDouble(1));
        var normalized = PilotEvalOracle.value(loaded, real);
        check(normalized.get("bits").equals("0000000000000001") && normalized.get("text").equals("4.9E-324"), "Double.MIN_VALUE");
        real.setValue(Double.longBitsToDouble(0x7ff0000000000001L));
        check(PilotEvalOracle.value(loaded, real).get("bits").equals("7ff8000000000000"), "canonical NaN");
        real.setValue(-0.0);
        check(PilotEvalOracle.value(loaded, real).get("bits").equals("8000000000000000"), "negative zero");
        var string = F.createLiteralString();
        string.setValue("a\n\"b");
        check(PilotEvalOracle.value(loaded, string).get("value").equals("a\n\"b"), "raw string");
        var bool = F.createLiteralBoolean();
        bool.setValue(true);
        check(PilotEvalOracle.value(loaded, bool).get("value").equals(true), "boolean");
        check(PilotEvalOracle.value(loaded, F.createLiteralInfinity()).equals(Map.of("kind", "infinity")), "infinity");
        check(PilotEvalOracle.outcome(loaded, () -> null).get("outcome").equals("not-evaluable"), "null list");
        check(PilotEvalOracle.outcome(loaded, List::of).get("values").equals(List.of()), "empty list");
        check(PilotEvalOracle.outcome(loaded, () -> java.util.Collections.singletonList(null)).get("outcome").equals("exception"), "null list item is not an empty list");
        check(PilotEvalOracle.outcome(loaded, () -> { throw new IllegalStateException("test"); }).get("outcome").equals("exception"), "exception captured");
        check(PilotEvalOracle.outcome(loaded, () -> { throw new StackOverflowError(); }).get("outcome").equals("exception"), "recursion captured");
        check(PilotEvalOracle.outcome(loaded, () -> List.of(integer)).get("outcome").equals("values"), "continue after failure");
        var feature = F.createFeature();
        check(PilotEvalOracle.value(loaded, feature).equals(Map.of("kind", "synthetic", "class", "Feature")), "synthetic element");
        var resource = new ResourceImpl(URI.createURI("memory:/test.sysml"));
        resource.getContents().add(feature);
        loaded.names.put(resource, "test.sysml");
        normalized = PilotEvalOracle.value(loaded, feature);
        check(normalized.get("kind").equals("element") && normalized.get("resource").equals("test.sysml")
            && normalized.get("fragment").equals(resource.getURIFragment(feature)), "contained identity");
    }

    private static void sites() {
        var resource = new ResourceImpl(URI.createURI("memory:/sites.sysml"));
        var namespace = F.createNamespace();
        resource.getContents().add(namespace);
        var type = F.createClass();
        own(namespace, F.createOwningMembership(), type);
        var feature = F.createFeature();
        own(type, F.createFeatureMembership(), feature);
        var expression = F.createOperatorExpression();
        own(feature, F.createFeatureValue(), expression);
        var parameter = F.createFeature();
        own(expression, F.createParameterMembership(), parameter);
        own(parameter, F.createFeatureValue(), F.createLiteralInteger());
        var function = F.createFunction();
        own(type, F.createOwningMembership(), function);
        var result = F.createLiteralInteger();
        own(function, F.createResultExpressionMembership(), result);
        var multiplicity = F.createMultiplicityRange();
        own(feature, F.createOwningMembership(), multiplicity);
        var bound = F.createLiteralInfinity();
        own(multiplicity, F.createOwningMembership(), bound);
        var sites = PilotEvalOracle.sites(resource);
        check(sites.size() == 3, "only outer expressions are sites");
        check(sites.get(0).expression() == expression && sites.get(0).target() == type, "feature-value target skips owning feature");
        check(sites.get(1).expression() == bound && sites.get(1).target() == feature, "bound uses owning namespace and containment order");
        check(sites.get(2).expression() == result && sites.get(2).target() == function, "result uses owning type");
        var untypedFeature = F.createFeature();
        own(namespace, F.createOwningMembership(), untypedFeature);
        var untypedValue = F.createLiteralInteger();
        own(untypedFeature, F.createFeatureValue(), untypedValue);
        check(PilotEvalOracle.site(untypedValue).target() == null, "package is not a type target");
    }

    private static void evaluation() {
        var loaded = new PilotEvalOracle.Loaded(new XtextResourceSet());
        var resource = new ResourceImpl(URI.createURI("memory:/literal.sysml"));
        loaded.names.put(resource, "literal.sysml");
        var literal = F.createLiteralInteger();
        literal.setValue(7);
        resource.getContents().add(literal);
        var row = PilotEvalOracle.evaluate(loaded, new PilotEvalOracle.Site(literal, null, "result-expression"));
        check(Boolean.TRUE.equals(row.get("evaluable")), "literal is model-level evaluable");
        var expected = Map.of("outcome", "values", "values", List.of(Map.of("kind", "int", "value", 7)));
        for (String mode : List.of("model_level", "gated", "extended")) {
            check(expected.equals(row.get(mode)), mode + " calls the real Pilot evaluator");
        }

        var namespace = F.createNamespace();
        resource.getContents().add(namespace);
        var pkg = F.createPackage();
        pkg.setDeclaredName("Example");
        own(namespace, F.createOwningMembership(), pkg);
        var function = F.createFunction();
        function.setDeclaredName("f");
        own(pkg, F.createOwningMembership(), function);
        var invocation = F.createInvocationExpression();
        own(pkg, F.createOwningMembership(), invocation);
        var reference = F.createMembership();
        invocation.getOwnedRelationship().add(reference);
        reference.setMemberElement(function);
        row = PilotEvalOracle.evaluate(loaded, new PilotEvalOracle.Site(invocation, null, "feature-value"));
        check("Example::f".equals(row.get("function")), "function selector uses Pilot's derived function and qualified name");
    }

    private static void isolation() {
        var loaded = new PilotEvalOracle.Loaded(new XtextResourceSet());
        var resource = new ResourceImpl(URI.createURI("memory:/isolation.sysml"));
        loaded.set.getResources().add(resource);
        loaded.names.put(resource, "isolation.sysml");
        var root = F.createNamespace();
        resource.getContents().add(root);
        var pkg = F.createPackage();
        own(root, F.createOwningMembership(), pkg);
        var source = F.createFeature();
        own(pkg, F.createOwningMembership(), source);
        var original = F.createInvocationExpression();
        var valuation = F.createFeatureValue();
        own(source, valuation, original);
        var reference = F.createFeatureReferenceExpression();
        own(pkg, F.createOwningMembership(), reference);
        var membership = F.createMembership();
        reference.getOwnedRelationship().add(membership);
        membership.setMemberElement(source);
        var function = F.createFunction();
        own(pkg, F.createOwningMembership(), function);
        var parameter = F.createFeature();
        parameter.setDirection(FeatureDirectionKind.IN);
        own(function, F.createParameterMembership(), parameter);
        loaded.captureOrigins();
        var identity = PilotEvalOracle.reference(loaded, original);
        check(!reference.isModelLevelEvaluable(), "source value starts not evaluable");
        for (boolean fail : List.of(false, true, false)) {
            var result = PilotEvalOracle.outcome(loaded, () -> {
                org.omg.sysml.util.EvaluationUtil.createInvocationOf(function, original);
                check(valuation.getValue() == null, "binding detaches the source expression during observation");
                check(reference.isModelLevelEvaluable(), "detachment changes a later evaluability query");
                if (fail) throw new StackOverflowError("test restoration");
                return List.of(original);
            });
            check(valuation.getValue() == original && original.eContainer() == valuation,
                "source containment restored after success and exception");
            check(!reference.isModelLevelEvaluable(), "derived queries see the restored feature value");
            check(PilotEvalOracle.reference(loaded, original).equals(identity), "source identity stays stable");
            if (fail) {
                check("exception".equals(result.get("outcome")), "exception remains observable");
            } else {
                var expected = new java.util.LinkedHashMap<String, Object>(identity);
                expected.put("kind", "element");
                check(List.of(expected).equals(result.get("values")), "moved source object is not mislabeled synthetic");
            }
        }
    }

    private static void existingModelExporter() {
        var loaded = new PilotEvalOracle.Loaded(new XtextResourceSet());
        var resource = new ResourceImpl(URI.createURI("memory:/export.kerml"));
        loaded.set.getResources().add(resource);
        loaded.names.put(resource, "export.kerml");
        var namespace = F.createNamespace();
        resource.getContents().add(namespace);
        namespace.setDeclaredName("ExportTest");
        var literal = F.createLiteralInteger();
        literal.setValue(7);
        own(namespace, F.createOwningMembership(), literal);
        loaded.captureOrigins();
        var exported = (List<?>) PilotEvalOracle.isolated(loaded, () -> PilotEvalModelExport.resources(loaded));
        check(exported.size() == 1, "existing exporter receives the supplied resource set");
        var entry = (Map<?, ?>) exported.get(0);
        check(entry.get("resource").equals("export.kerml"), "existing exporter receives normalized names");
        var objects = (List<?>) entry.get("objects");
        check(objects.size() == 3, "existing exporter retains complete containment");
        var value = (Map<?, ?>) objects.get(2);
        check(value.get("class").equals("LiteralInteger") &&
            ((Map<?, ?>) value.get("values")).get("value").equals(7), "existing exporter retains typed values");
        check(literal.eResource() == resource && namespace.getOwnedRelationship().size() == 1,
            "isolated export restores source containment");
    }

    public static void main(String[] args) {
        PilotOracle.setup();
        normalization();
        sites();
        evaluation();
        isolation();
        existingModelExporter();
        System.out.println(checks + " PilotEvalOracle milestone assertions passed");
    }
}
