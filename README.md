CLI tool for exporting encrypted messages from a room or direct message.

### Running
To run the program, use Nix:
```console
$ nix run
```
This lets you try the program once or twice without installing to `$PATH`.

To build, use:
```console
$ nix build
```
The output will be placed in `./result/`.

See the [website](https://nixos.org/download) to install, and the [wiki](https://wiki.nixos.org/wiki/Flakes) to enable flakes (required!).

## Current features
### Cross-signing support
Verification using emoji combinations, should allow downloading from rooms with "Only send messages to verified users" turned on.
Room key files alone also might not have everything one needs (See [#5] for details).

### Interactive prompts
Done using [promkit].

### Config file
For loading a preset, create `./met-config.toml`. An example is listed in [met-config.toml](met-config.toml).
Env vars with the prefix `MET_` can also be used, overriding the config. Example: `MET_USERID`.

### Multiple room exports
The selection prompt allows for more than one room. The downloads are concurrent.

### Caching export data
This is especially useful for larger exports, for example with 100k events.
The program creates `met-cache.json` on 10,000 messages downloaded. This file contains:
- Room ID
- Last message token used
- Room display name

The message token can be resumed from. Note that this is _not_ the event ID.
Every time the cache is updated, the last 10,000 messages/events get written too.

That means if you download 29k events and exit, only 20k are exported, but guarantees consistency when resuming later.

### Performance
After adding `mpsc::channel` for downloading and writing, I tested this on a room with >750k messages.
200k events were downloaded in 10 minutes over a laggy connection 1000 km away from the homeserver.

This first downloads 100 events, then filters them out to *decrypted* **text** messages.
The counter is for all events, so the real *message* count might be lower, as all other event types get ignored.
I plan on adding exports of media and other types, where this counting might be improved, when I'm done with the plans below.

### Roadmap
Plans for features/improvements (no eta):
- [ ] Add CLI arguments
    - Low priority.
- [ ] Switch for order of messages (chronologically or reverse)
    - Low priority.
- [x] Direct export to a file
    - [ ] Export in formats like json?
- [x] Improve fetching messages
- [x] *Silent mode*, import a preset file with account data
    - This is useful for periodic exports and debugging.

There's more, but these will be focused on first.

[#5]: https://github.com/kir68k/matrix-export-tool/issues/5
[promkit]: https://crates.io/crates/promkit
