/* Handwritten milestone-only oracle probes for representation edge cases.
 * Uses the pinned Pilot implementation, with no model parser or production JVM.
 */
import org.omg.sysml.lang.sysml.*;
import org.omg.sysml.util.EvaluationUtil;
import org.omg.sysml.expressions.ModelLevelExpressionEvaluator;
import org.omg.sysml.execution.expressions.functions.sequence.ExcludingAtFunction;
import org.omg.sysml.execution.expressions.functions.string.StringSubstringFunction;

public final class PilotEvalEdgeTest {
    private static final SysMLFactory F = SysMLFactory.eINSTANCE;
    private static int checks;
    private static void check(boolean condition, String message) {
        checks++;
        if (!condition) throw new AssertionError(message);
    }
    private static InvocationExpression call(Expression... arguments) {
        var function = F.createFunction();
        for (Expression argument : arguments) {
            var parameter = F.createFeature();
            parameter.setDirection(FeatureDirectionKind.IN);
            var membership = F.createParameterMembership();
            function.getOwnedRelationship().add(membership);
            membership.getOwnedRelatedElement().add(parameter);
        }
        return EvaluationUtil.createInvocationOf(function, arguments);
    }
    public static void main(String[] args) {
        PilotOracle.setup();
        var evaluator = ModelLevelExpressionEvaluator.INSTANCE;
        var split = call(EvaluationUtil.literalString("\ud83d\ude00"),
                         EvaluationUtil.literalInteger(1), EvaluationUtil.literalInteger(1));
        var result = new StringSubstringFunction().invoke(split, null, evaluator);
        check(result.size() == 1 && result.get(0) instanceof LiteralString, "substring result is a string");
        String text = ((LiteralString)result.get(0)).getValue();
        check(text.length() == 1 && text.charAt(0) == '\ud83d', "substring retains the lone high surrogate");
        var enumeration = EvaluationUtil.results(FeatureDirectionKind.IN);
        check(enumeration.size() == 1 && enumeration.get(0) == null, "enum reflection is a list containing Java null");
        var malformed = call(EvaluationUtil.literalInteger(9),
                             EvaluationUtil.literalString("bad"), EvaluationUtil.literalInteger(1));
        boolean threw = false;
        try { new ExcludingAtFunction().invoke(malformed, null, evaluator); }
        catch (ClassCastException expected) { threw = true; }
        check(threw, "excludingAt precedence admits a noninteger start and then throws");
        var omittedEnd = call(EvaluationUtil.literalInteger(9), EvaluationUtil.literalInteger(1));
        check(new ExcludingAtFunction().invoke(omittedEnd, null, evaluator).isEmpty(), "excludingAt omitted end removes only the start position");
        var sourceValue = F.createFeatureValue();
        var sourceExpression = F.createInvocationExpression();
        sourceValue.setValue(sourceExpression);
        call(sourceExpression);
        check(sourceValue.getValue() == null && sourceExpression.eContainer() != sourceValue,
            "Pilot argument binding moves an existing expression out of its source FeatureValue");
        System.out.println(checks + " Pilot evaluator edge assertions passed");
        System.out.println("Substring lone surrogate: U+D83D; reflective enum: [Java null]; excludingAt invalid start: ClassCastException");
    }
}
