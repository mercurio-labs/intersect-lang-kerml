/* Handwritten independent verification probe, never production.
 * Milestone-only: body Feature arguments are references, not frozen values.
 */
import java.util.List;
import org.eclipse.emf.common.util.URI;
import org.eclipse.emf.ecore.resource.impl.ResourceImpl;
import org.eclipse.xtext.resource.XtextResourceSet;
import org.omg.sysml.lang.sysml.*;
import org.omg.sysml.expressions.ModelLevelExpressionEvaluator;
import org.omg.sysml.util.EvaluationUtil;

public final class PilotEvalBindingTest {
    private static final SysMLFactory F = SysMLFactory.eINSTANCE;
    private static void own(Element owner, Relationship relationship, Element child) {
        owner.getOwnedRelationship().add(relationship);
        relationship.getOwnedRelatedElement().add(child);
    }
    private static FeatureReferenceExpression reference(Feature feature) {
        var expression = F.createFeatureReferenceExpression();
        var membership = F.createMembership();
        expression.getOwnedRelationship().add(membership);
        membership.setMemberElement(feature);
        return expression;
    }
    public static void main(String[] args) {
        PilotOracle.setup();
        int checks = 0;
        for (var evaluator : List.of(ModelLevelExpressionEvaluator.INSTANCE,
                org.omg.sysml.execution.expressions.ExpressionEvaluator.INSTANCE)) {
            var resource = new ResourceImpl(URI.createURI("memory:/bindings.kerml"));
            new XtextResourceSet().getResources().add(resource);
            var root = F.createNamespace();
            resource.getContents().add(root);
            var inner = F.createClass();
            own(root, F.createOwningMembership(), inner);
            var base = F.createFeature();
            base.setDeclaredName("base");
            own(inner, F.createFeatureMembership(), base);
            var outer = F.createFeature();
            own(root, F.createOwningMembership(), outer);
            var redefining = F.createFeature();
            redefining.setDeclaredName("redefining");
            own(outer, F.createFeatureMembership(), redefining);
            var redefinition = F.createRedefinition();
            redefining.getOwnedRelationship().add(redefinition);
            redefinition.setRedefinedFeature(base);
            redefinition.setRedefiningFeature(redefining);
            var valueMembership = F.createFeatureValue();
            valueMembership.setValue(EvaluationUtil.literalInteger(3));
            redefining.getOwnedRelationship().add(valueMembership);
            var body = F.createExpression();
            own(root, F.createOwningMembership(), body);
            var parameter = F.createFeature();
            parameter.setDeclaredName("parameter");
            parameter.setDirection(FeatureDirectionKind.IN);
            own(body, F.createParameterMembership(), parameter);
            own(body, F.createResultExpressionMembership(), reference(parameter));
            var result = evaluator.evaluateExpression(body, outer, base);
            if (result == null || result.size() != 1 || !(result.get(0) instanceof LiteralInteger value)
                    || value.getValue() != 3) throw new AssertionError("Feature argument must be referenced in body context: " + result);
            checks++;
        }
        System.out.println(checks + " Pilot body-binding assertions passed");
    }
}
