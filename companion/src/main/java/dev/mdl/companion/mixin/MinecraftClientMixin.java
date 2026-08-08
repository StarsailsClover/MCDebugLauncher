package dev.mdl.companion.mixin;

import dev.mdl.companion.MdlServer;
import net.minecraft.client.MinecraftClient;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

/**
 * Hooks into the client lifecycle:
 * - every client tick: lazily start the MDL control server (needs a live
 *   MinecraftClient instance, which entrypoints do not reliably have)
 * - shutdown: remove the runtime/agent.port discovery file
 */
@Mixin(MinecraftClient.class)
public abstract class MinecraftClientMixin {

    @Inject(method = "tick", at = @At("TAIL"))
    private void mdl$onTick(CallbackInfo ci) {
        // ensureStarted() is guarded and returns immediately after the
        // server thread has been spawned, so the per-tick cost is a
        // volatile read.
        MdlServer.ensureStarted((MinecraftClient) (Object) this);
    }

    @Inject(method = "scheduleStop", at = @At("HEAD"))
    private void mdl$onScheduleStop(CallbackInfo ci) {
        MdlServer.onShutdown((MinecraftClient) (Object) this);
    }
}
