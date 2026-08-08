package dev.mdl.companion;

import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import net.minecraft.client.MinecraftClient;

/**
 * Parses one JSON request line and dispatches it to {@link GameActions}.
 * Every request produces exactly one JSON response line.
 */
public final class Protocol {

    private Protocol() {
    }

    public static String handle(MinecraftClient client, String line) {
        try {
            JsonObject req = JsonParser.parseString(line).getAsJsonObject();
            String cmd = req.has("cmd") ? req.get("cmd").getAsString() : "";
            switch (cmd) {
                case "status":
                    return GameActions.status(client);
                case "key":
                    return GameActions.key(client, req);
                case "look":
                    return GameActions.look(client, req);
                case "click":
                    return GameActions.click(client, req);
                case "scroll":
                    return GameActions.scroll(client, req);
                case "chat":
                    return GameActions.chat(client, req);
                default:
                    return error("unknown command: " + cmd);
            }
        } catch (Exception e) {
            return error("bad request: " + e);
        }
    }

    public static String ok() {
        return "{\"status\":\"ok\"}";
    }

    public static String ok(JsonObject data) {
        data.addProperty("status", "ok");
        return data.toString();
    }

    public static String error(String message) {
        JsonObject o = new JsonObject();
        o.addProperty("status", "error");
        o.addProperty("message", message);
        return o.toString();
    }
}
