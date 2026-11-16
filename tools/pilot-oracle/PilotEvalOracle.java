/*
 * Handwritten independent verification oracle, never production.
 * Uses the pinned Pilot runtime and the same setup/resolve/validation sequence
 * as PilotOracle. Java is used only by this milestone verification tool.
 * Usage: PilotEvalOracle OUT.json LIBRARY_ROOT ROOT FILE...
 * LIBRARY_ROOT is the sysml.library directory; FILE paths are relative to ROOT.
 */
import com.google.gson.GsonBuilder;
import com.google.inject.Injector;
import java.io.Writer;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Collections;
import java.util.Comparator;
import java.util.IdentityHashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Set;
import java.util.function.Supplier;
import org.eclipse.emf.common.util.URI;
import org.eclipse.emf.ecore.EObject;
import org.eclipse.emf.ecore.change.util.ChangeRecorder;
import org.eclipse.emf.ecore.resource.Resource;
import org.eclipse.emf.ecore.util.EcoreUtil;
import org.eclipse.xtext.nodemodel.util.NodeModelUtils;
import org.eclipse.xtext.resource.XtextResource;
import org.eclipse.xtext.resource.XtextResourceSet;
import org.eclipse.xtext.util.CancelIndicator;
import org.eclipse.xtext.validation.CheckMode;
import org.eclipse.xtext.validation.IResourceValidator;
import org.eclipse.xtext.validation.Issue;
import org.omg.sysml.expressions.ModelLevelExpressionEvaluator;
import org.omg.sysml.lang.sysml.Element;
import org.omg.sysml.lang.sysml.Expression;
import org.omg.sysml.lang.sysml.FeatureValue;
import org.omg.sysml.lang.sysml.InvocationExpression;
import org.omg.sysml.lang.sysml.LiteralBoolean;
import org.omg.sysml.lang.sysml.LiteralInfinity;
import org.omg.sysml.lang.sysml.LiteralInteger;
import org.omg.sysml.lang.sysml.LiteralRational;
import org.omg.sysml.lang.sysml.LiteralString;
import org.omg.sysml.lang.sysml.MetadataFeature;
import org.omg.sysml.lang.sysml.MultiplicityRange;
import org.omg.sysml.lang.sysml.ResultExpressionMembership;
import org.omg.sysml.lang.sysml.Type;
import org.omg.sysml.util.ElementUtil;

public final class PilotEvalOracle {
    record Site(Expression expression, Element target, String kind) {}

    static final class Loaded {
        final XtextResourceSet set;
        final Map<Resource, String> names = new IdentityHashMap<>();
        final List<Resource> selected = new ArrayList<>();
        final List<Map<String, Object>> diagnostics = new ArrayList<>();
        final Map<Resource, List<Issue>> rawIssues = new IdentityHashMap<>();
        final Map<Element, Map<String, Object>> origins = new IdentityHashMap<>();
        Loaded(XtextResourceSet set) { this.set = set; }

        void captureOrigins() {
            for (Resource resource : set.getResources()) {
                resource.getAllContents().forEachRemaining(object -> {
                    if (object instanceof Element element) {
                        origins.putIfAbsent(element, fields("resource", resourceName(this, resource),
                            "fragment", resource.getURIFragment(element), "class", element.eClass().getName()));
                    }
                });
            }
        }
    }

    static Map<String, Object> fields(Object... items) {
        Map<String, Object> result = new LinkedHashMap<>();
        for (int i = 0; i < items.length; i += 2) {
            result.put((String) items[i], items[i + 1]);
        }
        return result;
    }

    static Resource load(Loaded loaded, Path path, String name) {
        Resource resource = loaded.set.getResource(URI.createFileURI(path.toAbsolutePath().toString()), true);
        loaded.names.put(resource, name.replace('\\', '/'));
        return resource;
    }

    static Loaded load(Injector injector, Path library, Path root, List<String> files) throws Exception {
        Loaded loaded = new Loaded(injector.getInstance(XtextResourceSet.class));
        List<Path> sources;
        try (var paths = Files.walk(library)) {
            sources = paths.filter(Files::isRegularFile)
                .filter(p -> p.toString().endsWith(".kerml") || p.toString().endsWith(".sysml"))
                .sorted(Comparator.comparing(p -> library.relativize(p).toString().replace('\\', '/')))
                .toList();
        }
        if (sources.isEmpty()) throw new IllegalArgumentException("No standard-library sources under " + library);
        for (Path source : sources) {
            load(loaded, source, "sysml.library/" + library.relativize(source));
        }
        for (String file : files) {
            Resource resource = load(loaded, root.resolve(file), file);
            if (loaded.selected.contains(resource)) throw new IllegalArgumentException("Duplicate input resource: " + file);
            loaded.selected.add(resource);
        }
        System.out.printf("Parsed %d library resources and %d selected resources%n", sources.size(), loaded.selected.size());
        resolveAndValidate(injector, loaded);
        return loaded;
    }

    static void resolveAndValidate(Injector injector, Loaded loaded) {
        loaded.diagnostics.clear();
        loaded.rawIssues.clear();
        EcoreUtil.resolveAll(loaded.set);
        System.out.printf("Resolved %d resources; validating%n", loaded.set.getResources().size());
        IResourceValidator fallback = injector.getInstance(IResourceValidator.class);
        // Validation can demand-load resources; follow PilotOracle.load's loop.
        for (int i = 0; i < loaded.set.getResources().size(); i++) {
            Resource resource = loaded.set.getResources().get(i);
            IResourceValidator validator = resource instanceof XtextResource xtext
                ? xtext.getResourceServiceProvider().getResourceValidator() : fallback;
            var issues = validator.validate(resource, CheckMode.ALL, CancelIndicator.NullImpl);
            loaded.rawIssues.put(resource, issues);
            for (var issue : issues) {
                loaded.diagnostics.add(fields("resource", resourceName(loaded, resource),
                    "severity", issue.getSeverity().toString().toLowerCase(Locale.ROOT),
                    "code", issue.getCode(), "message", issue.getMessage(),
                    "offset", issue.getOffset(), "length", issue.getLength()));
            }
            if ((i + 1) % 25 == 0) System.out.printf("Validated %d resources%n", i + 1);
        }
    }

    static String resourceName(Loaded loaded, Resource resource) {
        return loaded.names.getOrDefault(resource, resource.getURI().toString());
    }

    static Map<String, Object> reference(Loaded loaded, Element element) {
        Map<String, Object> origin = loaded.origins.get(element);
        if (origin != null) return origin;
        Resource resource = element.eResource();
        if (resource == null) throw new IllegalArgumentException("Reference has no resource: " + element.eClass().getName());
        return fields("resource", resourceName(loaded, resource),
            "fragment", resource.getURIFragment(element), "class", element.eClass().getName());
    }

    static Type ancestorType(EObject start) {
        for (EObject parent = start == null ? null : start.eContainer(); parent != null; parent = parent.eContainer()) {
            if (parent instanceof Type type) return type;
        }
        return null;
    }

    static Site site(Expression expression) {
        EObject parent = expression.eContainer();
        if (parent instanceof FeatureValue value && value.getValue() == expression) {
            return new Site(expression, ancestorType(value.getFeatureWithValue()), "feature-value");
        }
        if (parent instanceof ResultExpressionMembership result && result.getOwnedResultExpression() == expression) {
            return new Site(expression, ancestorType(parent), "result-expression");
        }
        if (parent != null && parent.eContainer() instanceof MultiplicityRange range
                && (range.getLowerBound() == expression || range.getUpperBound() == expression)) {
            return new Site(expression, range.getOwningNamespace(), "multiplicity-bound");
        }
        return null;
    }

    static List<Site> sites(Resource resource) {
        List<Site> result = new ArrayList<>();
        ArrayDeque<EObject> pending = new ArrayDeque<>();
        pushReverse(pending, resource.getContents());
        Set<EObject> seen = Collections.newSetFromMap(new IdentityHashMap<>());
        while (!pending.isEmpty()) {
            EObject object = pending.pop();
            if (!seen.add(object)) continue;
            if (object instanceof Expression expression) {
                Site site = site(expression);
                if (site != null) {
                    result.add(site);
                    continue; // A site's entire expression subtree belongs to it.
                }
            }
            pushReverse(pending, List.copyOf(object.eContents()));
        }
        return result;
    }

    private static void pushReverse(ArrayDeque<EObject> pending, List<? extends EObject> values) {
        for (int i = values.size() - 1; i >= 0; i--) pending.push(values.get(i));
    }

    static Map<String, Object> value(Loaded loaded, Element element) {
        if (element == null) throw new IllegalArgumentException("Pilot returned a null element inside a value sequence");
        if (element instanceof LiteralBoolean literal) return fields("kind", "bool", "value", literal.isValue());
        if (element instanceof LiteralInteger literal) return fields("kind", "int", "value", literal.getValue());
        if (element instanceof LiteralRational literal) {
            double value = literal.getValue();
            return fields("kind", "real", "bits", String.format(Locale.ROOT, "%016x", Double.doubleToLongBits(value)),
                "text", Double.toString(value));
        }
        if (element instanceof LiteralString literal) return fields("kind", "string", "value", literal.getValue());
        if (element instanceof LiteralInfinity) return fields("kind", "infinity");
        if (element instanceof MetadataFeature metadata && element.eResource() == null
                && !loaded.origins.containsKey(element)) {
            for (Element annotated : metadata.getAnnotatedElement()) {
                if (ElementUtil.getMetaclassFeatureFor(annotated) == metadata) {
                    return fields("kind", "metaclass", "of", reference(loaded, annotated), "class", annotated.eClass().getName());
                }
            }
        }
        if (element.eResource() != null || loaded.origins.containsKey(element)) {
            Map<String, Object> result = fields("kind", "element");
            result.putAll(reference(loaded, element));
            return result;
        }
        return fields("kind", "synthetic", "class", element.eClass().getName());
    }

    static Map<String, Object> outcome(Loaded loaded, Supplier<? extends List<? extends Element>> evaluate) {
        return isolated(loaded, () -> {
            try {
                List<? extends Element> values = evaluate.get();
                if (values == null) return fields("outcome", "not-evaluable");
                List<Map<String, Object>> normalized = new ArrayList<>();
                for (Element value : values) normalized.add(value(loaded, value));
                return fields("outcome", "values", "values", normalized);
            } catch (Exception | StackOverflowError error) {
                return exception(error);
            }
        });
    }

    /** Pilot argument binding can reparent source Expressions. Undo EMF changes
     * after each observation, including exceptions, so modes/sites are independent.
     * Serialize before undo, using captured source identities for moved objects.
     * A restoration failure escapes: continuing would produce contaminated data.
     */
    static <T> T isolated(Loaded loaded, Supplier<T> observation) {
        ChangeRecorder recorder = new ChangeRecorder(loaded.set);
        try {
            return observation.get();
        } finally {
            try {
                recorder.endRecording().applyAndReverse();
            } finally {
                recorder.dispose();
            }
        }
    }

    static Map<String, Object> exception(Throwable error) {
        return fields("outcome", "exception", "message", error.getClass().getName()
            + (error.getMessage() == null ? "" : ": " + error.getMessage()));
    }

    static Map<String, Object> evaluate(Loaded loaded, Site site) {
        Expression expression = site.expression();
        Map<String, Object> row = new LinkedHashMap<>(reference(loaded, expression));
        row.put("kind", site.kind());
        row.put("target", site.target() == null ? null : reference(loaded, site.target()));
        row.put("target_fragment", site.target() == null ? null : site.target().eResource().getURIFragment(site.target()));
        isolated(loaded, () -> {
            if (expression instanceof InvocationExpression invocation) {
                try {
                    var function = invocation.getFunction();
                    row.put("function", function == null ? null : function.getQualifiedName());
                } catch (Exception | StackOverflowError error) {
                    row.put("function", null);
                    row.put("function_error", exception(error));
                }
            }
            return null;
        });
        var node = NodeModelUtils.getNode(expression);
        if (node != null) row.put("source", fields("start", node.getOffset(), "end", node.getEndOffset(),
            "line", node.getStartLine(), "offset_encoding", "utf-16"));
        isolated(loaded, () -> {
            try {
                row.put("evaluable", expression.isModelLevelEvaluable());
            } catch (Exception | StackOverflowError error) {
                row.put("evaluable", null);
                row.put("evaluable_error", exception(error));
            }
            return null;
        });
        row.put("model_level", outcome(loaded, () -> ModelLevelExpressionEvaluator.INSTANCE.evaluate(expression, site.target())));
        row.put("gated", outcome(loaded, () -> expression.evaluate(site.target())));
        row.put("extended", outcome(loaded, () -> org.omg.sysml.execution.expressions.ExpressionEvaluator.INSTANCE.evaluate(expression, site.target())));
        return row;
    }

    public static void main(String[] args) throws Exception {
        Path modelOut = null;
        if (args.length >= 2 && args[0].equals("--model-out")) {
            modelOut = Path.of(args[1]);
            args = java.util.Arrays.copyOfRange(args, 2, args.length);
        }
        if (args.length < 4) throw new IllegalArgumentException("Usage: PilotEvalOracle OUT.json LIBRARY_ROOT ROOT FILE...");
        if (Runtime.version().feature() < 19) throw new IllegalStateException("JDK 19 or later is required (use the documented JDK 21)");
        Path out = Path.of(args[0]);
        Path library = Path.of(args[1]).toRealPath();
        Path root = Path.of(args[2]).toRealPath();
        Loaded loaded = load(PilotOracle.setup(), library, root, List.of(args).subList(3, args.length));
        if (modelOut != null) {
            loaded.captureOrigins();
            Object resources = isolated(loaded, () -> PilotEvalModelExport.resources(loaded));
            try (Writer writer = Files.newBufferedWriter(modelOut, StandardCharsets.UTF_8)) {
                new GsonBuilder().disableHtmlEscaping().serializeNulls().create().toJson(
                    fields("oracle", "unchanged PilotOracle.export; fresh evaluation load", "resources", resources), writer);
            }
        }
        writeReport(loaded, out);
    }

    static void writeReport(Loaded loaded, Path out) throws Exception {
        loaded.captureOrigins();
        List<Map<String, Object>> rows = new ArrayList<>();
        // Freeze the site list before evaluation can create synthetic objects.
        List<Site> sites = new ArrayList<>();
        for (Resource resource : loaded.selected) sites.addAll(sites(resource));
        System.out.printf("Evaluating %d sites with isolated observations%n", sites.size());
        for (Site site : sites) {
            rows.add(evaluate(loaded, site));
            if (rows.size() % 10 == 0) System.out.printf("Evaluated %d sites%n", rows.size());
        }
        Map<String, Object> report = fields("oracle", "SysML v2 Pilot Implementation 2026-08",
            "observation_isolation", "EMF ChangeRecorder undo per evaluability query and mode; captured source identities",
            "revision", "692170b71867353b8f90341e61556f49a5beb0e5", "java", System.getProperty("java.version"),
            "resources", loaded.selected.stream().map(r -> resourceName(loaded, r)).toList(),
            "diagnostics", loaded.diagnostics, "sites", rows);
        try (Writer writer = Files.newBufferedWriter(out, StandardCharsets.UTF_8)) {
            new GsonBuilder().disableHtmlEscaping().serializeNulls().create().toJson(report, writer);
        }
        System.out.printf("Recorded %d sites in %d files; %d load diagnostics%n", rows.size(), loaded.selected.size(), loaded.diagnostics.size());
    }
}
