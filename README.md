CLI tool for exporting encrypted messages from a room or direct message.

### Current features
There aren't that many atm :p
- Interactive prompts\
Done using [promkit]
- Config file\
For loading a preset, create `./met-config.toml`. An example is listed in [met-config.toml](met-config.toml).\
Env vars with the prefix `MET_` can also be used, overriding the config. Example: `MET_USERID`.
- Cross-signing support\
Verification using emoji combinations, should allow downloading from rooms with "Only send messages to verified users" turned on.\
Room key files alone also might not have everything one needs (See [#5] for details).
- File output\
Concurrently downloads messages and writes them to a file.

### Running
This uses Nix (flake) as the build system. To build, use:
```console
$ nix build
```
The output will be placed in `./result/`.

To run the program, use:
```console
$ nix run
```
This also lets you try the program once or twice without installing to `$PATH`.

See the [website](https://nixos.org/download) to install, and the [wiki](https://wiki.nixos.org/wiki/Flakes) to enable flakes (required).

### Performance
After adding `mpsc::channel` for downloading and writing, I tested this on a room with >750k messages.
200k events were downloaded in 10 minutes.

Update: This was over a 2.4GHz Wi-Fi connection 1000 km away from my server :p
I haven't thoroughly tested this yet, but it's better than this on a good connection.

Currently, this first downloads chunks of events, then filters them out to *decrypted* **text** messages.
The counter is for all events, so the real *message* count might be lower, as all other event types get ignored.
I plan on adding exports of media and other types, where this counting might be improved, when I'm done with the plans below.

### Roadmap
Plans for features/improvements (no eta):
- [ ] Add CLI arguments
- [ ] Switch for order of messages (chronologically or reverse)\
Low priority.
- [x] Direct export to a file
    - [ ] Export in formats like json
- [x] Improve fetching messages
- [x] *Silent mode*, import a preset file with account data\
This is useful for periodic exports and debugging.

There's more, but these will be focused on first.

[#5]: https://github.com/kir68k/matrix-export-tool/issues/5
[promkit]: https://crates.io/crates/promkit
