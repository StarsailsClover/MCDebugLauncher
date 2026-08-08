package dev.mdl.companion;

import com.google.gson.JsonObject;
import net.minecraft.client.MinecraftClient;
import net.minecraft.client.gui.screen.GameMenuScreen;
import net.minecraft.client.gui.screen.Screen;
import net.minecraft.client.option.GameOptions;
import net.minecraft.client.option.KeyBinding;
import net.minecraft.client.util.InputUtil;
import net.minecraft.client.network.ClientPlayerEntity;

import java.util.HashMap;
import java.util.Map;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.TimeUnit;

/**
 * Executes agent commands inside the game.
 *
 * Input is injected through Minecraft's own keybinding/input systems on the
 * client thread, which is why it works without window focus and never moves
 * the user's real mouse or keyboard:
 *
 * - movement / attack / use  -> {@link KeyBinding#setKeyPressed} feeds the
 *   same {@code KeyboardInput} + {@code handleInputEvents} path real input uses
 * - view rotation            -> player yaw/pitch setters
 * - chat / commands          -> the player's network handler
 * - GUI clicks               -> the open screen's mouseClicked handler
 */
public final class GameActions {

    /** Scheduled executor for delayed key releases ("tap" actions). */
    private static final java.util.concurrent.ScheduledExecutorService SCHEDULER =
            java.util.concurrent.Executors.newSingleThreadScheduledExecutor(r -> {
                Thread t = new Thread(r, "MDL-Agent-Scheduler");
                t.setDaemon(true);
                return t;
            });

    private GameActions() {
    }

    // ------------------------------------------------------------------
    // Thread marshalling
    // ------------------------------------------------------------------

    /** Run an action on the client thread and wait for its result. */
    private static String onClientThread(MinecraftClient client, java.util.function.Supplier<String> action) {
        if (client.isOnThread()) {
            return action.get();
        }
        CompletableFuture<String> future = new CompletableFuture<>();
        client.execute(() -> {
            try {
                future.complete(action.get());
            } catch (Exception e) {
                future.complete(Protocol.error(e.toString()));
            }
        });
        try {
            return future.get(10, TimeUnit.SECONDS);
        } catch (Exception e) {
            return Protocol.error("action timed out: " + e);
        }
    }

    // ------------------------------------------------------------------
    // Keybinding resolution
    // ------------------------------------------------------------------

    private static KeyBinding resolveBinding(MinecraftClient client, String name) {
        GameOptions o = client.options;
        Map<String, KeyBinding> byName = new HashMap<>();
        byName.put("forward", o.forwardKey);
        byName.put("w", o.forwardKey);
        byName.put("back", o.backKey);
        byName.put("s", o.backKey);
        byName.put("left", o.leftKey);
        byName.put("a", o.leftKey);
        byName.put("right", o.rightKey);
        byName.put("d", o.rightKey);
        byName.put("jump", o.jumpKey);
        byName.put("space", o.jumpKey);
        byName.put("sneak", o.sneakKey);
        byName.put("shift", o.sneakKey);
        byName.put("sprint", o.sprintKey);
        byName.put("ctrl", o.sprintKey);
        byName.put("attack", o.attackKey);
        byName.put("use", o.useKey);
        byName.put("pickitem", o.pickItemKey);
        byName.put("drop", o.dropKey);
        byName.put("inventory", o.inventoryKey);
        byName.put("e", o.inventoryKey);
        byName.put("chat", o.chatKey);
        byName.put("t", o.chatKey);
        byName.put("command", o.commandKey);
        byName.put("playerlist", o.playerListKey);
        byName.put("tab", o.playerListKey);
        byName.put("advancements", o.advancementsKey);
        byName.put("swapoffhand", o.swapHandsKey);
        byName.put("saveToolbarActivator", o.saveToolbarActivatorKey);
        byName.put("loadToolbarActivator", o.loadToolbarActivatorKey);
        for (int i = 0; i < 9; i++) {
            byName.put("hotbar" + (i + 1), o.hotbarKeys[i]);
            byName.put(String.valueOf(i + 1), o.hotbarKeys[i]);
        }
        return byName.get(name.toLowerCase());
    }

    private static InputUtil.Key keyOf(KeyBinding binding) {
        return binding.getDefaultKey();
    }

    // ------------------------------------------------------------------
    // Commands
    // ------------------------------------------------------------------

    public static String status(MinecraftClient client) {
        return onClientThread(client, () -> {
            JsonObject o = new JsonObject();
            boolean inWorld = client.world != null && client.player != null;
            o.addProperty("in_world", inWorld);
            o.addProperty("paused", client.isPaused());
            o.addProperty("focused", client.isWindowFocused());
            o.addProperty("screen", client.currentScreen == null ? null : client.currentScreen.getClass().getSimpleName());
            if (inWorld) {
                ClientPlayerEntity p = client.player;
                JsonObject pos = new JsonObject();
                pos.addProperty("x", p.getX());
                pos.addProperty("y", p.getY());
                pos.addProperty("z", p.getZ());
                pos.addProperty("yaw", p.getYaw());
                pos.addProperty("pitch", p.getPitch());
                o.add("player", pos);
            }
            o.addProperty("mdl_protocol", MdlCompanionMod.PROTOCOL_VERSION);
            o.addProperty("companion_version", MdlCompanionMod.MOD_VERSION);
            return Protocol.ok(o);
        });
    }

    public static String key(MinecraftClient client, JsonObject req) {
        String key = req.has("key") ? req.get("key").getAsString() : "";
        String action = req.has("action") ? req.get("action").getAsString() : "tap";
        long holdMs = req.has("hold_ms") ? req.get("hold_ms").getAsLong() : 60;

        if (key.isEmpty()) {
            return Protocol.error("missing 'key'");
        }

        // Escape is special: it opens/closes menus and is not a KeyBinding.
        if (key.equalsIgnoreCase("escape") || key.equalsIgnoreCase("esc")) {
            return onClientThread(client, () -> {
                if (client.currentScreen != null) {
                    client.currentScreen.keyPressed(256, 0, 0); // GLFW_KEY_ESCAPE
                } else {
                    client.setScreen(new GameMenuScreen(false));
                }
                return Protocol.ok();
            });
        }

        KeyBinding binding = resolveBinding(client, key);
        if (binding == null) {
            return Protocol.error("unknown key: " + key);
        }
        final InputUtil.Key boundKey = keyOf(binding);

        switch (action.toLowerCase()) {
            case "press":
                return onClientThread(client, () -> {
                    KeyBinding.setKeyPressed(boundKey, true);
                    return Protocol.ok();
                });
            case "release":
                return onClientThread(client, () -> {
                    KeyBinding.setKeyPressed(boundKey, false);
                    return Protocol.ok();
                });
            case "tap":
                String pressResult = onClientThread(client, () -> {
                    KeyBinding.setKeyPressed(boundKey, true);
                    return Protocol.ok();
                });
                if (pressResult.contains("\"error\"")) {
                    return pressResult;
                }
                final long delay = Math.max(20, holdMs);
                SCHEDULER.schedule(() -> {
                    try {
                        client.execute(() -> KeyBinding.setKeyPressed(boundKey, false));
                    } catch (Exception ignored) {
                    }
                }, delay, TimeUnit.MILLISECONDS);
                return Protocol.ok();
            default:
                return Protocol.error("unknown action: " + action + " (use press|release|tap)");
        }
    }

    public static String look(MinecraftClient client, JsonObject req) {
        float yaw = req.has("yaw") ? req.get("yaw").getAsFloat() : 0f;
        float pitch = req.has("pitch") ? req.get("pitch").getAsFloat() : 0f;
        boolean relative = req.has("relative") && req.get("relative").getAsBoolean();

        return onClientThread(client, () -> {
            if (client.player == null) {
                return Protocol.error("player not in world");
            }
            ClientPlayerEntity p = client.player;
            float targetYaw = relative ? p.getYaw() + yaw : yaw;
            float targetPitch = relative ? p.getPitch() + pitch : pitch;
            targetPitch = Math.max(-90f, Math.min(90f, targetPitch));
            p.setYaw(targetYaw);
            p.setPitch(targetPitch);
            return Protocol.ok();
        });
    }

    public static String click(MinecraftClient client, JsonObject req) {
        String button = req.has("button") ? req.get("button").getAsString() : "left";
        String action = req.has("action") ? req.get("action").getAsString() : "tap";
        boolean hasX = req.has("x");
        boolean hasY = req.has("y");
        long holdMs = req.has("hold_ms") ? req.get("hold_ms").getAsLong() : 60;

        // GUI click: when a screen is open and coordinates are given, click
        // the screen directly (works even without window focus).
        final boolean guiClick = hasX && hasY;
        final double x = hasX ? req.get("x").getAsDouble() : 0;
        final double y = hasY ? req.get("y").getAsDouble() : 0;
        final int btnCode = button.equalsIgnoreCase("right") ? 1 : button.equalsIgnoreCase("middle") ? 2 : 0;

        KeyBinding binding;
        switch (button.toLowerCase()) {
            case "left":
                binding = client.options.attackKey;
                break;
            case "right":
                binding = client.options.useKey;
                break;
            case "middle":
                binding = client.options.pickItemKey;
                break;
            default:
                return Protocol.error("unknown button: " + button + " (use left|right|middle)");
        }
        final InputUtil.Key boundKey = keyOf(binding);

        switch (action.toLowerCase()) {
            case "press":
                return onClientThread(client, () -> {
                    if (guiClick && client.currentScreen != null) {
                        client.currentScreen.mouseClicked(x, y, btnCode);
                        return Protocol.ok();
                    }
                    KeyBinding.setKeyPressed(boundKey, true);
                    return Protocol.ok();
                });
            case "release":
                return onClientThread(client, () -> {
                    if (guiClick && client.currentScreen != null) {
                        client.currentScreen.mouseReleased(x, y, btnCode);
                        return Protocol.ok();
                    }
                    KeyBinding.setKeyPressed(boundKey, false);
                    return Protocol.ok();
                });
            case "tap":
                String pressResult = onClientThread(client, () -> {
                    if (guiClick && client.currentScreen != null) {
                        client.currentScreen.mouseClicked(x, y, btnCode);
                        client.currentScreen.mouseReleased(x, y, btnCode);
                        return Protocol.ok();
                    }
                    KeyBinding.setKeyPressed(boundKey, true);
                    return Protocol.ok();
                });
                if (pressResult.contains("\"error\"")) {
                    return pressResult;
                }
                if (!(guiClick)) {
                    final long delay = Math.max(20, holdMs);
                    SCHEDULER.schedule(() -> {
                        try {
                            client.execute(() -> KeyBinding.setKeyPressed(boundKey, false));
                        } catch (Exception ignored) {
                        }
                    }, delay, TimeUnit.MILLISECONDS);
                }
                return Protocol.ok();
            default:
                return Protocol.error("unknown action: " + action + " (use press|release|tap)");
        }
    }

    public static String scroll(MinecraftClient client, JsonObject req) {
        double amount = req.has("amount") ? req.get("amount").getAsDouble() : 0;
        if (amount == 0) {
            return Protocol.error("scroll amount must be non-zero");
        }

        return onClientThread(client, () -> {
            if (client.player == null) {
                return Protocol.error("player not in world");
            }
            // Use vanilla hotbar scroll logic so behavior matches the real wheel,
            // then sync the selected slot to the server.
            var inventory = client.player.getInventory();
            int size = net.minecraft.entity.player.PlayerInventory.getHotbarSize();
            int current = inventory.selectedSlot;
            int delta = (int) Math.round(-amount); // wheel up (positive) = previous slot
            int next = ((current + delta) % size + size) % size;
            inventory.setSelectedSlot(next);
            if (client.player.networkHandler != null) {
                client.player.networkHandler.sendPacket(
                        new net.minecraft.network.packet.c2s.play.UpdateSelectedSlotC2SPacket(
                                inventory.selectedSlot));
            }
            return Protocol.ok();
        });
    }

    public static String chat(MinecraftClient client, JsonObject req) {
        String message = req.has("message") ? req.get("message").getAsString() : "";
        if (message.isEmpty()) {
            return Protocol.error("missing 'message'");
        }

        return onClientThread(client, () -> {
            if (client.player == null || client.player.networkHandler == null) {
                return Protocol.error("player not connected");
            }
            if (message.startsWith("/")) {
                client.player.networkHandler.sendCommand(message.substring(1));
            } else {
                client.player.networkHandler.sendChatMessage(message);
            }
            return Protocol.ok();
        });
    }
}
