/*
 * Handwritten independent verification oracle (never production code, never
 * part of the normal build). Loads KerML library files with the SysML v2
 * Pilot Implementation (release 2026-08, the corpus's pinned commit) into one
 * resource set, resolves every proxy, and writes a normalized JSON export:
 * each object's class, source span, stored values, children and resolved
 * reference targets, with load timings.
 *
 * Usage: java -cp <pilot-all.jar>;<classes> PilotOracle OUT.json ROOT FILE...
 * FILE paths are relative to ROOT and are reported as given.
 *
 * Example mode: PilotOracle --examples OUT.json MANIFEST.json loads the
 * manifest's library files once, then each example folder in turn on top of
 * them (see ExampleRun below) and writes per-file diagnostics, unresolved
 * references and object counts instead of the full object export.
 */
import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.inject.Injector;
import java.io.File;
import java.io.Writer;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.IdentityHashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import org.eclipse.emf.common.util.EList;
import org.eclipse.emf.common.util.Enumerator;
import org.eclipse.emf.common.util.URI;
import org.eclipse.emf.ecore.EAttribute;
import org.eclipse.emf.ecore.EObject;
import org.eclipse.emf.ecore.EReference;
import org.eclipse.emf.ecore.EStructuralFeature;
import org.eclipse.emf.ecore.resource.Resource;
import org.eclipse.emf.ecore.util.EcoreUtil;
import org.eclipse.xtext.nodemodel.ICompositeNode;
import org.eclipse.xtext.nodemodel.util.NodeModelUtils;
import org.eclipse.xtext.resource.XtextResourceSet;
import org.eclipse.xtext.validation.CheckMode;
import org.eclipse.xtext.validation.IResourceValidator;
import org.eclipse.xtext.validation.Issue;
import org.eclipse.xtext.util.CancelIndicator;
import org.omg.kerml.xtext.KerMLStandaloneSetup;

public final class PilotOracle {
    private static final class Loaded {
        XtextResourceSet set;
        Map<Resource, String> names = new IdentityHashMap<>();
        double parseMs;
        double resolveMs;
        double validateMs;
        Map<Resource, List<Issue>> diagnostics = new IdentityHashMap<>();
    }

    static double ms(long start) {
        return (System.nanoTime() - start) / 1e6;
    }

    private static Loaded load(Injector injector, File root, List<String> files) {
        Loaded loaded = new Loaded();
        loaded.set = injector.getInstance(XtextResourceSet.class);
        long start = System.nanoTime();
        for (String file : files) {
            URI uri = URI.createFileURI(new File(root, file).getAbsolutePath());
            Resource resource = loaded.set.getResource(uri, true);
            loaded.names.put(resource, file);
        }
        loaded.parseMs = ms(start);
        start = System.nanoTime();
        EcoreUtil.resolveAll(loaded.set);
        loaded.resolveMs = ms(start);
        start = System.nanoTime();
        IResourceValidator fallback = injector.getInstance(IResourceValidator.class);
        // Validation may demand-load another resource; validate those too.
        // Each resource is validated by its own language (KerML or SysML).
        for (int index = 0; index < loaded.set.getResources().size(); index++) {
            Resource resource = loaded.set.getResources().get(index);
            IResourceValidator validator = resource instanceof org.eclipse.xtext.resource.XtextResource xtext
                    ? xtext.getResourceServiceProvider().getResourceValidator()
                    : fallback;
            loaded.diagnostics.put(resource,
                    validator.validate(resource, CheckMode.ALL, CancelIndicator.NullImpl));
        }
        loaded.validateMs = ms(start);
        return loaded;
    }

    static Injector setup() {
        // The metamodel package registers itself on first access.
        org.eclipse.emf.ecore.EPackage.Registry.INSTANCE.put(
                org.omg.sysml.lang.sysml.SysMLPackage.eNS_URI, org.omg.sysml.lang.sysml.SysMLPackage.eINSTANCE);
        Injector injector = new KerMLStandaloneSetup().createInjectorAndDoEMFRegistration();
        // Registers the `.sysml` resource factory, so SysML files load too.
        new org.omg.sysml.xtext.SysMLStandaloneSetup().createInjectorAndDoEMFRegistration();
        return injector;
    }

    public static void main(String[] args) throws Exception {
        if (args.length == 3 && args[0].equals("--examples")) {
            ExampleRun.main(Path.of(args[1]), Path.of(args[2]));
            return;
        }
        if (args.length < 3) {
            System.err.println("Usage: PilotOracle OUT.json ROOT FILE...");
            System.exit(2);
        }
        Path out = Path.of(args[0]);
        File root = new File(args[1]);
        List<String> files = List.of(args).subList(2, args.length);

        long start = System.nanoTime();
        Injector injector = setup();
        double setupMs = ms(start);

        Loaded cold = load(injector, root, files);
        Loaded warm = load(injector, root, files);

        Map<String, Object> report = new LinkedHashMap<>();
        report.put("oracle", "SysML v2 Pilot Implementation 2026-08 (jupyter-sysml-kernel-0.62.0-all.jar)");
        report.put("java", System.getProperty("java.version"));
        Map<String, Object> timing = new LinkedHashMap<>();
        timing.put("setup_ms", setupMs);
        timing.put("cold_parse_ms", cold.parseMs);
        timing.put("cold_resolve_ms", cold.resolveMs);
        timing.put("cold_validate_ms", cold.validateMs);
        timing.put("warm_parse_ms", warm.parseMs);
        timing.put("warm_resolve_ms", warm.resolveMs);
        timing.put("warm_validate_ms", warm.validateMs);
        report.put("timing", timing);
        report.put("resources", export(cold));
        try (Writer writer = Files.newBufferedWriter(out, StandardCharsets.UTF_8)) {
            Gson gson = new GsonBuilder().disableHtmlEscaping().serializeNulls().create();
            gson.toJson(report, writer);
        }
        System.out.printf(
                "setup %.0f ms; cold parse %.0f ms, resolve %.0f ms, validate %.0f ms; warm parse %.0f ms, resolve %.0f ms, validate %.0f ms%n",
                setupMs, cold.parseMs, cold.resolveMs, cold.validateMs,
                warm.parseMs, warm.resolveMs, warm.validateMs);
    }

    /** An object's portable identity: its resource and EMF fragment. */
    private static Map<String, Object> ref(Loaded loaded, EObject object) {
        return ref(loaded.names, object);
    }

    static Map<String, Object> ref(Map<Resource, String> names, EObject object) {
        Map<String, Object> ref = new LinkedHashMap<>();
        if (object.eIsProxy()) {
            ref.put("unresolved", EcoreUtil.getURI(object).toString());
            return ref;
        }
        Resource resource = object.eResource();
        ref.put("resource", resource == null ? null : names.getOrDefault(resource, resource.getURI().toString()));
        ref.put("fragment", resource == null ? null : resource.getURIFragment(object));
        return ref;
    }

    private static Object value(Object value) {
        if (value instanceof Enumerator e) {
            return e.getLiteral();
        }
        if (value instanceof Boolean || value instanceof Number || value instanceof String || value == null) {
            return value;
        }
        return value.toString();
    }

    private static List<Object> export(Loaded loaded) {
        List<Object> resources = new ArrayList<>();
        for (Resource resource : new ArrayList<>(loaded.set.getResources())) {
            Map<String, Object> entry = new LinkedHashMap<>();
            entry.put("resource", loaded.names.getOrDefault(resource, resource.getURI().toString()));
            List<String> errors = new ArrayList<>();
            for (Resource.Diagnostic d : resource.getErrors()) {
                errors.add(d.getLine() + ": " + d.getMessage());
            }
            entry.put("errors", errors);
            List<Object> diagnostics = new ArrayList<>();
            for (Issue issue : loaded.diagnostics.getOrDefault(resource, List.of())) {
                Map<String, Object> diagnostic = new LinkedHashMap<>();
                diagnostic.put("severity", issue.getSeverity().toString().toLowerCase(java.util.Locale.ROOT));
                diagnostic.put("code", issue.getCode());
                diagnostic.put("message", issue.getMessage());
                diagnostic.put("target", null);
                diagnostic.put("start", null);
                diagnostic.put("end", null);
                URI uri = issue.getUriToProblem();
                EObject target = uri == null ? null : loaded.set.getEObject(uri, false);
                if (target != null) {
                    diagnostic.put("target", ref(loaded, target));
                    ICompositeNode node = NodeModelUtils.getNode(target);
                    if (node != null) {
                        diagnostic.put("start", node.getOffset());
                        diagnostic.put("end", node.getEndOffset());
                    }
                } else if (uri != null) {
                    Resource targetResource = loaded.set.getResource(uri.trimFragment(), false);
                    Map<String, Object> targetRef = new LinkedHashMap<>();
                    targetRef.put("resource", loaded.names.getOrDefault(targetResource, uri.trimFragment().toString()));
                    targetRef.put("fragment", uri.fragment());
                    diagnostic.put("target", targetRef);
                }
                // The issue can point at a feature token rather than the whole node.
                diagnostic.put("offset", issue.getOffset());
                diagnostic.put("length", issue.getLength());
                diagnostics.add(diagnostic);
            }
            entry.put("diagnostics", diagnostics);
            List<Object> objects = new ArrayList<>();
            resource.getAllContents().forEachRemaining(object -> objects.add(object(loaded, resource, object)));
            entry.put("objects", objects);
            resources.add(entry);
        }
        return resources;
    }

    private static Map<String, Object> object(Loaded loaded, Resource resource, EObject object) {
        return object(loaded.names, resource, object);
    }

    static Map<String, Object> object(Map<Resource, String> names, Resource resource, EObject object) {
        Map<String, Object> entry = new LinkedHashMap<>();
        entry.put("fragment", resource.getURIFragment(object));
        entry.put("class", object.eClass().getName());
        ICompositeNode node = NodeModelUtils.getNode(object);
        if (node != null) {
            entry.put("start", node.getOffset());
            entry.put("end", node.getEndOffset());
        }
        Map<String, Object> values = new LinkedHashMap<>();
        for (EStructuralFeature feature : object.eClass().getEAllStructuralFeatures()) {
            if (feature.isDerived() || feature.isTransient() || feature.isVolatile() || !object.eIsSet(feature)) {
                continue;
            }
            if (feature instanceof EReference reference && reference.isContainer()) {
                continue;
            }
            Object raw = object.eGet(feature, false);
            if (feature instanceof EAttribute) {
                if (raw instanceof EList<?> list) {
                    List<Object> items = new ArrayList<>();
                    for (Object item : list) {
                        items.add(value(item));
                    }
                    values.put(feature.getName(), items);
                } else {
                    values.put(feature.getName(), value(raw));
                }
            } else if (raw instanceof EList<?> list) {
                List<Object> items = new ArrayList<>();
                for (Object item : list) {
                    items.add(ref(names, (EObject) item));
                }
                values.put(feature.getName(), items);
            } else if (raw instanceof EObject target) {
                values.put(feature.getName(), ref(names, target));
            }
        }
        entry.put("values", values);
        return entry;
    }
}
