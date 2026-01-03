# met
Graphical tool for exporting most common event types from matrix rooms. Currently, this supports:
- **Cross-signing** (verification)
  - Supported methods: Emoji/SAS
  - Planned methods: QR (scanned on another device)
- **Session storage**
  - By default, upon login a client session directory will be made inside `$XDG_DATA_HOME`. On linux, this will normally be `~/.local/share/io.github.kir68k.met/`.
  - Secrets are saved and retrieved using [keyring_core], which will pick a platform-dependent secret store.
    - Note for linux: This requires a working secret manager like keepassxc. Desktop environments provide this by default, so it only matters if you're not using one.
  - In the future, an ephemeral mode without storage might be added.
- **Concurrent exporting**
  - By default, the program will start downloading concurrently from all of the selected rooms.
  - Customizing (restricting or disabling) this behavior is planned.

### Exporting
The program will export the following event types, automatically decrypting if necessary:
- `text/[user] - messages.txt`: Text messages, 1 timestamped message per line.
- `media/[category]`: Media files, split into separate directories.
- `media/Avatars/{Rooms,Users}`: The current and any historical room/user avatars found in the TL.
- `events/[user] - Events.json`: All decrypted event data
- `events/[user] - Unable to decrypt.json`: All utd event data

### Libraries used
- [matrix_sdk]: The official matrix library
- [gpui]: GUI library from Zed
- [gpui_component]: UI components, titlebar
- [keyring_core]: Native secret management
- [mimalloc]: A performant allocator

## Running
To run the program, either build it with cargo, or use Nix.
Note that Nix might require `nixGL` to load the ui on non-nixos.

### Dependencies
- X11/Wayland/DBus (linux)
- Vulkan

## Other info
### Caching export data
This is especially useful for larger exports, for example with 100k events.
The program creates `met-cache.json` on 10,000 messages downloaded. This file contains:
- Room ID
- Last message token used
- Room display name

The message token can be resumed from. Note that this is _not_ the event ID.
Every time the cache is updated, the last 10,000 messages/events get written too.

That means if you download 29k events and exit, only 20k are exported, but guarantees consistency when resuming later.

## Performance
> [!info] This might be outdated.
> I'll have to test the speeds again as most of the program
> has changed after moving onto a graphical interface.
>
> The switch to mimalloc makes me curious about both memory usage,
> and how it changes during a large export, and maybe the speed
> (assuming network isn't the limitation).

After adding `mpsc::channel` for downloading and writing, I tested this on a room with >750k messages.
200k events were downloaded in 10 minutes over a laggy connection 1000 km away from the homeserver.

This first downloads 100 events, then filters them out to *decrypted* **text** messages.
The counter is for all events, so the real *message* count might be lower, as all other event types get ignored.
I plan on adding exports of media and other types, where this counting might be improved, when I'm done with the plans below.

## Roadmap
Plans for features/improvements (no eta):
- [ ] Rework caching
    - The current `met-cache.json` setup feels a bit hacky to imo
- [ ] Customizing exports
    - This would be a good addition.
- [ ] A progress bar ... ¿¿?
    - I don't know if this is possible with how the timeline works. It sure would be nice though.
- [ ] Switch for order of messages (chronologically or reverse)
    - Low priority.
- [x] Direct export to a file
    - [x] Export in formats like json?
      - This is ... kinda done?

[matrix_sdk]: https://crates.io/crates/matrix_sdk
[gpui]: https://crates.io/crates/gpui
[gpui_component]: https://crates.io/crates/gpui-component
[keyring_core]: https://crates.io/crates/keyring-core
[mimalloc]: https://crates.io/crates/mimalloc
