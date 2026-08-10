# Alpha 9

## New
- **Instance info command** (`mdl instance-info <name>`) — displays detailed instance information including Minecraft version, loader type/version, disk usage, and content counts (mods, resource packs, shader packs)
- **JSON output support** — `--format json` flag provides machine-readable output for automation and monitoring

## Improved
- **Better instance monitoring** — easily track instance size and content without manual directory inspection
- **Automation-friendly** — JSON format enables scripting and integration with other tools

## Verified
- End-to-end: instance-info with both text and JSON output formats
- Tested with Fabric, Forge, and vanilla instances
- Disk usage calculation includes all subdirectories
- Content counting accurately reflects mod/resourcepack/shaderpack files

## Usage Examples
```bash
# View instance details in human-readable format
mdl instance-info my-instance

# Get JSON output for scripting
mdl instance-info my-instance --format json
```
