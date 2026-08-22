import com.sun.tools.attach.VirtualMachine;

/**
 * AttachHelper - loads a Java agent into a running JVM via the Attach API.
 *
 * Invoked by MDL as:
 *   java -cp <dir> AttachHelper <pid> <agentJarPath> [agentParams]
 *
 * Prints "OK" on success; on failure prints the error to stderr and exits 1.
 */
public class AttachHelper {
    public static void main(String[] args) {
        if (args.length < 2) {
            System.err.println("Usage: AttachHelper <pid> <agentJar> [params]");
            System.exit(2);
        }
        String pid = args[0];
        String agentJar = args[1];
        // Empty-string params must be passed through so agentmain sees them;
        // VirtualMachine.loadAgent(jar, null) is legal too but some agents
        // expect an empty string rather than null.
        String params = args.length >= 3 ? args[2] : "";
        try {
            VirtualMachine vm = VirtualMachine.attach(pid);
            try {
                vm.loadAgent(agentJar, params);
            } finally {
                vm.detach();
            }
            System.out.println("OK");
        } catch (Throwable t) {
            System.err.println("ATTACH_FAILED: " + t.getClass().getName() + ": " + t.getMessage());
            Throwable cause = t.getCause();
            while (cause != null) {
                System.err.println("  caused by: " + cause);
                cause = cause.getCause();
            }
            System.exit(1);
        }
    }
}
