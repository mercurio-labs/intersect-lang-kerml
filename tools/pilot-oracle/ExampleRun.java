/*
 * Handwritten independent verification oracle (never production code, never
 * part of the normal build). The example mode of PilotOracle: runs the SysML
 * v2 Pilot over example folders and reports what the Pilot itself says about
 * each file.
 *
 * The manifest is JSON:
 *   {"library_root": DIR, "library_files": [REL...],
 *    "examples_root": DIR, "folders": [{"folder": NAME, "files": [REL...]}]}
 *    (optional "warm_repeat": true runs every folder again in the same JVM and
 *    reports it as "warm_folders")
 *
 * The library files are loaded once into one Xtext resource set. Each folder's
 * files are then added to that set, every proxy of the folder's resources is
 * resolved (EcoreUtil.resolveAll per resource; library proxies resolve on
 * demand), and every non-library resource is validated with CheckMode.ALL by
 * its own language. Afterwards the folder's resources (and any resource
 * loaded on demand for it) are unloaded and removed, so each folder sees only
 * the library, as if loaded alone with it.
 *
 * Per file the report has the resource errors and warnings (the linker's
 * "Couldn't resolve reference to ..." diagnostics, with the reference feature,
 * the owning object and the link text found from the node model), the
 * validator issues (severity, code, message, line), the object count, and
 * per folder the parse, resolve and validate times.
 *
 * Each validator issue also names its target object (resource, EMF fragment,
 * class and node span, in UTF-16 offsets like the issue's own offset). With
 * "export_objects": true in the manifest, each file entry also carries the
 * object export of PilotOracle as "object_export" (class, span, stored values, references), so
 * the validation corpus can be paired with an Intersect dump by containment.
 */
import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import com.google.inject.Injector;
import java.io.File;
import java.io.Writer;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Collections;
import java.util.IdentityHashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Set;
import org.eclipse.emf.common.util.URI;
import org.eclipse.emf.ecore.EObject;
import org.eclipse.emf.ecore.EReference;
import org.eclipse.emf.ecore.resource.Resource;
import org.eclipse.emf.ecore.util.EcoreUtil;
import org.eclipse.xtext.CrossReference;
import org.eclipse.xtext.GrammarUtil;
import org.eclipse.xtext.diagnostics.AbstractDiagnostic;
import org.eclipse.xtext.nodemodel.ICompositeNode;
import org.eclipse.xtext.nodemodel.INode;
import org.eclipse.xtext.nodemodel.util.NodeModelUtils;
import org.eclipse.xtext.resource.XtextResource;
import org.eclipse.xtext.resource.XtextResourceSet;
import org.eclipse.xtext.util.CancelIndicator;
import org.eclipse.xtext.validation.CheckMode;
import org.eclipse.xtext.validation.IResourceValidator;
import org.eclipse.xtext.validation.Issue;

final class ExampleRun {
    private ExampleRun() {}

    private static List<String> strings(JsonArray array) {
        List<String> out = new ArrayList<>();
        for (JsonElement element : array) {
            out.add(element.getAsString());
        }
        return out;
    }

    static void main(Path out, Path manifestPath) throws Exception {
        JsonObject manifest = JsonParser.parseString(Files.readString(manifestPath, StandardCharsets.UTF_8))
                .getAsJsonObject();
        File libraryRoot = new File(manifest.get("library_root").getAsString());
        File examplesRoot = new File(manifest.get("examples_root").getAsString());

        long start = System.nanoTime();
        Injector injector = PilotOracle.setup();
        double setupMs = PilotOracle.ms(start);

        XtextResourceSet set = injector.getInstance(XtextResourceSet.class);
        Map<Resource, String> names = new IdentityHashMap<>();
        start = System.nanoTime();
        for (String file : strings(manifest.getAsJsonArray("library_files"))) {
            URI uri = URI.createFileURI(new File(libraryRoot, file).getAbsolutePath());
            names.put(set.getResource(uri, true), file);
        }
        double libraryParseMs = PilotOracle.ms(start);
        Set<Resource> library = Collections.newSetFromMap(new IdentityHashMap<>());
        library.addAll(set.getResources());
        int libraryErrors = 0;
        for (Resource resource : library) {
            libraryErrors += resource.getErrors().size();
        }
        System.out.printf("setup %.0f ms; library parse %.0f ms (%d resources, %d resource errors)%n",
                setupMs, libraryParseMs, library.size(), libraryErrors);

        boolean exportObjects = manifest.has("export_objects") && manifest.get("export_objects").getAsBoolean();
        List<Object> folders = new ArrayList<>();
        for (JsonElement element : manifest.getAsJsonArray("folders")) {
            JsonObject folder = element.getAsJsonObject();
            folders.add(runFolder(set, names, library, examplesRoot,
                    folder.get("folder").getAsString(), strings(folder.getAsJsonArray("files")), exportObjects));
        }
        // "warm_repeat": true runs every folder a second time in the same JVM
        // (classes loaded, JIT warm, library proxies already resolved), for the
        // warm Pilot column of the gate G6 timing.
        List<Object> warmFolders = new ArrayList<>();
        if (manifest.has("warm_repeat") && manifest.get("warm_repeat").getAsBoolean()) {
            System.out.println("warm repeat");
            for (JsonElement element : manifest.getAsJsonArray("folders")) {
                JsonObject folder = element.getAsJsonObject();
                warmFolders.add(runFolder(set, names, library, examplesRoot,
                        folder.get("folder").getAsString(), strings(folder.getAsJsonArray("files")), false));
            }
        }

        Map<String, Object> report = new LinkedHashMap<>();
        report.put("oracle", "SysML v2 Pilot Implementation 2026-08 (jupyter-sysml-kernel-0.62.0-all.jar)");
        report.put("java", System.getProperty("java.version"));
        report.put("library_resources", library.size());
        report.put("library_resource_errors", libraryErrors);
        Map<String, Object> timing = new LinkedHashMap<>();
        timing.put("setup_ms", setupMs);
        timing.put("library_parse_ms", libraryParseMs);
        report.put("timing", timing);
        report.put("folders", folders);
        if (!warmFolders.isEmpty()) {
            report.put("warm_folders", warmFolders);
        }
        try (Writer writer = Files.newBufferedWriter(out, StandardCharsets.UTF_8)) {
            Gson gson = new GsonBuilder().disableHtmlEscaping().serializeNulls().setPrettyPrinting().create();
            gson.toJson(report, writer);
        }
    }

    private static Map<String, Object> runFolder(XtextResourceSet set, Map<Resource, String> names,
            Set<Resource> library, File examplesRoot, String folderName, List<String> files,
            boolean exportObjects) {
        Map<String, Object> entry = new LinkedHashMap<>();
        entry.put("folder", folderName);
        List<Resource> own = new ArrayList<>();
        Map<Resource, List<Issue>> issues = new IdentityHashMap<>();
        try {
            long start = System.nanoTime();
            for (String file : files) {
                URI uri = URI.createFileURI(new File(examplesRoot, file).getAbsolutePath());
                Resource resource = set.getResource(uri, true);
                names.put(resource, file);
                own.add(resource);
            }
            double parseMs = PilotOracle.ms(start);
            start = System.nanoTime();
            for (Resource resource : own) {
                EcoreUtil.resolveAll(resource);
            }
            double resolveMs = PilotOracle.ms(start);
            start = System.nanoTime();
            for (int index = 0; index < set.getResources().size(); index++) {
                Resource resource = set.getResources().get(index);
                if (library.contains(resource)) {
                    continue;
                }
                IResourceValidator validator = ((XtextResource) resource).getResourceServiceProvider()
                        .getResourceValidator();
                issues.put(resource, validator.validate(resource, CheckMode.ALL, CancelIndicator.NullImpl));
            }
            double validateMs = PilotOracle.ms(start);
            Map<String, Object> timing = new LinkedHashMap<>();
            timing.put("parse_ms", parseMs);
            timing.put("resolve_ms", resolveMs);
            timing.put("validate_ms", validateMs);
            entry.put("timing", timing);
        } catch (RuntimeException | StackOverflowError e) {
            entry.put("exception", e.toString());
        }
        List<Object> fileEntries = new ArrayList<>();
        List<Resource> extra = new ArrayList<>();
        for (Resource resource : new ArrayList<>(set.getResources())) {
            if (library.contains(resource)) {
                continue;
            }
            fileEntries.add(fileEntry(set, names, resource, issues.getOrDefault(resource, List.of()),
                    own.contains(resource), exportObjects));
            extra.add(resource);
        }
        entry.put("files", fileEntries);
        int errors = 0;
        int warnings = 0;
        for (Object file : fileEntries) {
            @SuppressWarnings("unchecked")
            Map<String, Object> f = (Map<String, Object>) file;
            errors += (Integer) f.get("error_count");
            warnings += (Integer) f.get("warning_count");
        }
        entry.put("error_count", errors);
        entry.put("warning_count", warnings);
        System.out.printf("%s: %d files, %d errors, %d warnings %s%n", folderName, own.size(), errors, warnings,
                entry.getOrDefault("timing", entry.get("exception")));
        for (Resource resource : extra) {
            resource.unload();
            names.remove(resource);
        }
        set.getResources().removeAll(extra);
        return entry;
    }

    private static Map<String, Object> fileEntry(XtextResourceSet set, Map<Resource, String> names,
            Resource resource, List<Issue> issues, boolean requested, boolean exportObjects) {
        Map<String, Object> entry = new LinkedHashMap<>();
        entry.put("resource", names.getOrDefault(resource, resource.getURI().toString()));
        entry.put("requested", requested);
        int[] objects = {0};
        resource.getAllContents().forEachRemaining(object -> objects[0]++);
        entry.put("objects", objects[0]);
        List<Object> resourceDiagnostics = new ArrayList<>();
        Set<String> seen = new java.util.HashSet<>();
        int errors = 0;
        int warnings = 0;
        for (Resource.Diagnostic d : resource.getErrors()) {
            seen.add(key(d));
            resourceDiagnostics.add(resourceDiagnostic(resource, d, "error"));
            errors++;
        }
        for (Resource.Diagnostic d : resource.getWarnings()) {
            seen.add(key(d));
            resourceDiagnostics.add(resourceDiagnostic(resource, d, "warning"));
            warnings++;
        }
        entry.put("resource_diagnostics", resourceDiagnostics);
        List<Object> diagnostics = new ArrayList<>();
        for (Issue issue : issues) {
            String severity = issue.getSeverity().toString().toLowerCase(Locale.ROOT);
            // The validator repeats the resource's syntax and linking errors.
            if (issue.isSyntaxError() || seen.contains(issue.getOffset() + "|" + issue.getMessage())) {
                continue;
            }
            Map<String, Object> diagnostic = new LinkedHashMap<>();
            diagnostic.put("severity", severity);
            diagnostic.put("code", issue.getCode());
            diagnostic.put("message", issue.getMessage());
            diagnostic.put("line", issue.getLineNumber());
            diagnostic.put("column", issue.getColumn());
            diagnostic.put("offset", issue.getOffset());
            diagnostic.put("length", issue.getLength());
            URI uri = issue.getUriToProblem();
            EObject target = uri == null ? null : set.getEObject(uri, false);
            if (target != null) {
                diagnostic.put("target_class", target.eClass().getName());
                diagnostic.put("target_resource", target.eResource() == null ? null
                        : names.getOrDefault(target.eResource(), target.eResource().getURI().toString()));
                diagnostic.put("target_fragment", target.eResource() == null ? null
                        : target.eResource().getURIFragment(target));
                ICompositeNode node = NodeModelUtils.getNode(target);
                diagnostic.put("target_start", node == null ? null : node.getOffset());
                diagnostic.put("target_end", node == null ? null : node.getEndOffset());
            } else if (uri != null) {
                diagnostic.put("target_uri", uri.toString());
            }
            diagnostics.add(diagnostic);
            if (severity.equals("error")) {
                errors++;
            } else if (severity.equals("warning")) {
                warnings++;
            }
        }
        entry.put("diagnostics", diagnostics);
        entry.put("error_count", errors);
        entry.put("warning_count", warnings);
        if (exportObjects) {
            List<Object> exported = new ArrayList<>();
            resource.getAllContents().forEachRemaining(
                    object -> exported.add(PilotOracle.object(names, resource, object)));
            entry.put("object_export", exported);
        }
        return entry;
    }

    private static String key(Resource.Diagnostic d) {
        Integer offset = d instanceof AbstractDiagnostic diagnostic ? diagnostic.getOffset() : null;
        return offset + "|" + d.getMessage();
    }

    private static Map<String, Object> resourceDiagnostic(Resource resource, Resource.Diagnostic d,
            String severity) {
        Map<String, Object> entry = new LinkedHashMap<>();
        entry.put("severity", severity);
        entry.put("message", d.getMessage());
        entry.put("line", d.getLine());
        entry.put("column", d.getColumn());
        entry.put("kind", d.getClass().getSimpleName());
        if (d instanceof AbstractDiagnostic diagnostic) {
            entry.put("code", diagnostic.getCode());
            entry.put("offset", diagnostic.getOffset());
            entry.put("length", diagnostic.getLength());
            linkDetails(resource, diagnostic.getOffset(), entry);
        }
        return entry;
    }

    /** The cross-reference at the diagnostic: its feature, owner and link text. */
    private static void linkDetails(Resource resource, int offset, Map<String, Object> entry) {
        if (!(resource instanceof XtextResource xtext) || xtext.getParseResult() == null) {
            return;
        }
        ICompositeNode root = xtext.getParseResult().getRootNode();
        INode node = NodeModelUtils.findLeafNodeAtOffset(root, offset);
        while (node != null && !(node.getGrammarElement() instanceof CrossReference)) {
            node = node.getParent();
        }
        if (node == null) {
            return;
        }
        EReference reference = GrammarUtil.getReference((CrossReference) node.getGrammarElement());
        entry.put("feature", reference == null ? null : reference.getName());
        entry.put("link_text", NodeModelUtils.getTokenText(node));
        EObject owner = NodeModelUtils.findActualSemanticObjectFor(node);
        if (owner != null) {
            entry.put("owner_class", owner.eClass().getName());
            entry.put("owner_fragment", resource.getURIFragment(owner));
        }
    }
}
