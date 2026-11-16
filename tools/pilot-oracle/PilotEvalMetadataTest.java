/* Handwritten independent verification probes, never production.
 * Milestone-only: invokes the pinned Pilot JVM on in-memory M5 fixtures.
 */
import java.util.List;
import org.eclipse.emf.common.util.URI;
import org.eclipse.emf.ecore.resource.impl.ResourceImpl;
import org.eclipse.xtext.resource.XtextResourceSet;
import org.omg.sysml.lang.sysml.*;
import org.omg.sysml.expressions.ModelLevelExpressionEvaluator;
import org.omg.sysml.util.ElementUtil;
import org.omg.sysml.util.EvaluationUtil;
import org.omg.sysml.util.ExpressionUtil;
import org.omg.sysml.util.SysMLLibraryUtil;

public final class PilotEvalMetadataTest {
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

    private static void metadataAndReflection() {
        var resource = new ResourceImpl(URI.createURI("memory:/metadata.sysml"));
        new XtextResourceSet().getResources().add(resource);
        var root = F.createNamespace();
        resource.getContents().add(root);
        var part = F.createPartDefinition();
        part.setDeclaredName("P");
        part.setIsAbstract(true);
        own(root, F.createOwningMembership(), part);
        // Supply the one library identity this in-memory reflection fixture
        // needs; evaluation and metadata navigation still use the real Pilot.
        var partMetaclass = F.createMetaclass();
        partMetaclass.setDeclaredName("PartDefinition");
        own(root, F.createOwningMembership(), partMetaclass);
        SysMLLibraryUtil.setProviderLookup(r -> (context, name) ->
            name.equals("SysML::Systems::PartDefinition") ? partMetaclass : null);
        var member = F.createMetadataUsage();
        own(part, F.createOwningMembership(), member);
        var annotated = F.createMetadataFeature();
        var annotation = F.createAnnotation();
        annotation.setAnnotatedElement(part);
        own(part, annotation, annotated);
        check(ElementUtil.getAllMetadataFeaturesOf(part).equals(List.of(annotated, member)),
            "owned annotations precede owned metadata members");
        check(annotation.getAnnotatedElement() == part && annotation.getOwnedAnnotatingElement() == annotated,
            "the owned annotation fixture retains both endpoints");
        check(annotated.getOwningRelationship() == annotation,
            "the metadata is contained by the annotation");
        // In the pinned Pilot, owningAnnotatingRelationship uses the
        // Relationship container delegate rather than Element's. Consequently
        // this child does not see the containing Annotation in getAnnotation().
        check(ElementUtil.getAnnotatedElementOf(annotated).isEmpty(),
            "pinned Pilot omits a containing annotation from the metadata targets");
        check(ElementUtil.getAnnotatedElementOf(member).equals(List.of(part)),
            "owned metadata implicitly annotates its namespace");

        var access = F.createMetadataAccessExpression();
        var reference = F.createMembership();
        access.getOwnedRelationship().add(reference);
        reference.setMemberElement(part);
        own(root, F.createOwningMembership(), access);
        var evaluator = ModelLevelExpressionEvaluator.INSTANCE;
        var metaclass = ElementUtil.getMetaclassFeatureFor(part);
        check(metaclass != null, "fixture provides the PartDefinition library metaclass");
        check(evaluator.evaluate(access, null).equals(List.of(annotated, member, metaclass)),
            "metadata access appends the metaclass last");
        var field = F.createFeature();
        field.setDeclaredName("isAbstract");
        own(root, F.createOwningMembership(), field);
        var values = evaluator.evaluateFeature(field, metaclass);
        check(values.size() == 1 && values.get(0) instanceof LiteralBoolean value && value.isValue(),
            "metaclass reflection reads a stored Boolean");
        var name = F.createFeature();
        name.setDeclaredName("declaredName");
        own(root, F.createOwningMembership(), name);
        values = evaluator.evaluateFeature(name, metaclass);
        check(values.size() == 1 && values.get(0) instanceof LiteralString value && value.getValue().equals("P"),
            "metaclass reflection reads a stored string");
    }

    private static void conditionsAndBounds() {
        var yes = EvaluationUtil.literalBoolean(true);
        var no = EvaluationUtil.literalBoolean(false);
        check(ExpressionUtil.checkConditionOn(null, yes), "true satisfies a condition");
        check(!ExpressionUtil.checkConditionOn(null, no), "false does not satisfy a condition");
        check(!ExpressionUtil.checkConditionOn(null, F.createNullExpression()), "empty is not true");
        check(!ExpressionUtil.checkConditionOn(null, EvaluationUtil.literalInteger(1)), "integer one is not true");
        var range = F.createMultiplicityRange();
        var owner = F.createFeature();
        own(owner, F.createOwningMembership(), range);
        check(range.valueOf(EvaluationUtil.literalInteger(0)) == 0, "zero is a natural bound");
        check(range.valueOf(F.createLiteralInfinity()) == -1, "infinity uses the -1 sentinel");
        check(range.valueOf(EvaluationUtil.literalInteger(-1)) == -2, "negative bound is invalid");
        check(range.valueOf(EvaluationUtil.literalRational(1.5)) == -2, "real bound is invalid");
        check(range.valueOf(F.createNullExpression()) == -2, "empty bound is invalid");
    }

    public static void main(String[] args) {
        PilotOracle.setup();
        metadataAndReflection();
        conditionsAndBounds();
        System.out.println(checks + " Pilot metadata/condition assertions passed");
    }
}
