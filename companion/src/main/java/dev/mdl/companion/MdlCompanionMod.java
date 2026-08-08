package dev.mdl.companion;

import net.fabricmc.api.ClientModInitializer;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * MDL Agent Companion — entry point.
 *
 * The actual control server is started lazily on the first client tick
 * (see {@link MdlTick#tick}), because the Minecraft client instance and the
 * game directory are not fully available during entrypoint initialization.
 */
public class MdlCompanionMod implements ClientModInitializer {
    public static final String MOD_ID = "mdl-agent-companion";
    public static final String MOD_VERSION = "1.0.0";
    public static final int PROTOCOL_VERSION = 1;
    public static final Logger LOGGER = LoggerFactory.getLogger("MDL Companion");

    @Override
    public void onInitializeClient() {
        LOGGER.info("[MDL] Companion mod loaded (protocol v{}). Control server starts on first tick.", PROTOCOL_VERSION);
    }
}
