package dev.mdl.companion.mixin;

import net.minecraft.client.MinecraftClient;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

/**
 * When the launcher starts the game in agent mode it passes
 * {@code -Dmdl.agent.keepFocus=true}. In that mode the client is told its
 * window is always focused, so injected input keeps working while the user
 * works in other applications.
 *
 * Without this, Minecraft's input pipeline silently drops movement and use
 * actions the moment the window loses focus — even with pauseOnLostFocus off
 * — which would break background agent control.
 */
@Mixin(MinecraftClient.class)
public abstract class MinecraftClientFocusMixin {

    private static final boolean KEEP_FOCUS = Boolean.getBoolean("mdl.agent.keepFocus");

    @Inject(method = "isWindowFocused", at = @At("HEAD"), cancellable = true)
    private void mdl$keepFocus(CallbackInfoReturnable<Boolean> cir) {
        if (KEEP_FOCUS) {
            cir.setReturnValue(true);
        }
    }
}
