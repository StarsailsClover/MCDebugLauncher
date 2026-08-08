package dev.mdl.companion;

import net.minecraft.client.MinecraftClient;

import java.io.File;
import java.net.ServerSocket;
import java.net.Socket;
import java.net.InetAddress;

/**
 * TCP control server running inside the game process.
 *
 * Protocol: newline-delimited JSON (one request line -> one response line).
 * Binds to 127.0.0.1 only. The port is taken from the {@code mdl.agent.port}
 * JVM property set by the launcher (default 25590); the actually bound port
 * is written to {@code <gameDir>/runtime/agent.port} for discovery.
 *
 * All game mutations are marshalled onto the client (render) thread via
 * {@link MinecraftClient#execute(Runnable)} and their results are returned
 * through a future, so each request gets exactly one JSON response.
 */
public final class MdlServer {
    private static volatile boolean started = false;
    private static volatile int boundPort = -1;

    private MdlServer() {
    }

    public static void ensureStarted(MinecraftClient client) {
        if (started) return;
        synchronized (MdlServer.class) {
            if (started) return;
            started = true;
        }

        Thread thread = new Thread(() -> run(client), "MDL-Agent-Server");
        thread.setDaemon(true);
        thread.start();
    }

    public static int getBoundPort() {
        return boundPort;
    }

    private static void run(MinecraftClient client) {
        int requested = Integer.getInteger("mdl.agent.port", 25590);

        ServerSocket serverSocket = null;
        // Try the requested port first, then a small range, then any free port.
        int[] candidates = new int[22];
        for (int i = 0; i < 20; i++) candidates[i] = requested + i;
        candidates[20] = requested;
        candidates[21] = 0;

        for (int port : candidates) {
            try {
                serverSocket = new ServerSocket(port, 8, InetAddress.getLoopbackAddress());
                break;
            } catch (Exception e) {
                MdlCompanionMod.LOGGER.debug("[MDL] Port {} unavailable: {}", port, e.getMessage());
            }
        }

        if (serverSocket == null) {
            MdlCompanionMod.LOGGER.error("[MDL] Could not bind the agent control server; control disabled");
            return;
        }

        boundPort = serverSocket.getLocalPort();
        writePortFile(client, boundPort);
        MdlCompanionMod.LOGGER.info("[MDL] Agent control server listening on 127.0.0.1:{}", boundPort);

        try {
            while (!serverSocket.isClosed()) {
                Socket socket = serverSocket.accept();
                // Handle connections sequentially; commands are fast and this
                // keeps thread usage minimal inside the game process.
                handleConnection(client, socket);
            }
        } catch (Exception e) {
            if (!serverSocket.isClosed()) {
                MdlCompanionMod.LOGGER.error("[MDL] Agent server loop ended: {}", e.toString());
            }
        }
    }

    private static void handleConnection(MinecraftClient client, Socket socket) {
        try (socket;
             var reader = new java.io.BufferedReader(new java.io.InputStreamReader(socket.getInputStream(), java.nio.charset.StandardCharsets.UTF_8));
             var writer = new java.io.OutputStreamWriter(socket.getOutputStream(), java.nio.charset.StandardCharsets.UTF_8)) {
            socket.setTcpNoDelay(true);
            String line;
            while ((line = reader.readLine()) != null) {
                line = line.trim();
                if (line.isEmpty()) continue;
                String response = Protocol.handle(client, line);
                writer.write(response);
                writer.write('\n');
                writer.flush();
            }
        } catch (Exception e) {
            MdlCompanionMod.LOGGER.debug("[MDL] Agent connection ended: {}", e.toString());
        }
    }

    private static void writePortFile(MinecraftClient client, int port) {
        try {
            File gameDir = client.runDirectory;
            File runtime = new File(gameDir, "runtime");
            if (!runtime.exists() && !runtime.mkdirs()) {
                MdlCompanionMod.LOGGER.warn("[MDL] Could not create runtime dir at {}", runtime);
                return;
            }
            File portFile = new File(runtime, "agent.port");
            java.nio.file.Files.writeString(portFile.toPath(), Integer.toString(port));
        } catch (Exception e) {
            MdlCompanionMod.LOGGER.warn("[MDL] Failed to write agent.port: {}", e.toString());
        }
    }

    /** Remove the port file on shutdown so stale ports are not discovered. */
    public static void onShutdown(MinecraftClient client) {
        try {
            File portFile = new File(new File(client.runDirectory, "runtime"), "agent.port");
            if (portFile.exists()) {
                //noinspection ResultOfMethodCallIgnored
                portFile.delete();
            }
        } catch (Exception ignored) {
        }
    }
}
