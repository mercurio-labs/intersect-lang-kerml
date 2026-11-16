/* Handwritten verification-only adapter, never production.
 * Reuses the protected G5 export implementation without copying or changing it.
 * Reflection is confined to this pinned, local verification tool. Any API-shape
 * mismatch fails the run; there is no replacement exporter or silent fallback.
 */
import java.lang.reflect.InvocationTargetException;

final class PilotEvalModelExport {
    private PilotEvalModelExport() {}

    static Object resources(PilotEvalOracle.Loaded loaded) {
        try {
            Class<?> type = Class.forName("PilotOracle$Loaded");
            var constructor = type.getDeclaredConstructor();
            constructor.setAccessible(true);
            Object input = constructor.newInstance();
            for (var entry : java.util.Map.of(
                    "set", loaded.set, "names", loaded.names, "diagnostics", loaded.rawIssues).entrySet()) {
                var field = type.getDeclaredField(entry.getKey());
                field.setAccessible(true);
                field.set(input, entry.getValue());
            }
            var export = PilotOracle.class.getDeclaredMethod("export", type);
            export.setAccessible(true);
            return export.invoke(null, input);
        } catch (InvocationTargetException error) {
            throw new IllegalStateException("Existing PilotOracle export failed", error.getCause());
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException("Pinned PilotOracle export API changed", error);
        }
    }
}
