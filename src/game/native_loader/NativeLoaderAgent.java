/**
 * NativeLoaderAgent - loads a native library into the target JVM via
 * System.load() during premain/agentmain (v26.4-alpha.2).
 *
 * Replaces CreateRemoteThread DLL injection for JVM targets: modern JDKs
 * (25+) with CFG/CET mitigations crash on remote threads before DllMain
 * runs, while the JVM's own agent-attach + System.load() channel is the
 * vendor-expected deployment path and bypasses those mitigations entirely.
 *
 * Invoked by MDL as an attach agent with the DLL path as the agent args:
 *   VirtualMachine.attach(pid).loadAgent(nativeLoaderJar, dllPath)
 *
 * Both entrypoints delegate to the same loader; a failure aborts with a
 * non-zero exit-style exception so the attach surfaces a clear error.
 */
public class NativeLoaderAgent {
    public static void premain(String args) {
        load(args);
    }

    public static void agentmain(String args) {
        load(args);
    }

    static void load(String args) {
        if (args == null || args.trim().isEmpty()) {
            throw new IllegalArgumentException(
                "NativeLoaderAgent: missing DLL path in agent args");
        }
        System.load(args.trim());
    }
}
