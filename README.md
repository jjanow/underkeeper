# Underkeeper

A character editor for **Underkeep** save files. Set your heroes' attributes,
level, experience, health, mana, stamina, zeal, skill points, skill levels and
traits to whatever you like, so the gear you want to wear isn't locked behind
hidden stat requirements (looking at you, Blood Mages and Paladins and your
WIS-gated rings and amulets).

- Single self-contained program for **Windows, Linux and macOS**. Nothing to install.
- Modern GUI with **dark and light mode** (follows your system by default).
- Works with the **Steam and GOG** versions of the game.
- Finds your saves automatically: Windows, Linux (native Steam, Flatpak, Snap,
  Proton) and macOS (CrossOver / Whisky bottles), including GOG Galaxy saves.
- **Gear requirements** view: every item's STR / DEX / END / WIS / class
  requirements, read from your own game install, checked against the selected hero.
  One click raises a hero's stats to meet them.
- **Updates itself** from this repository's GitHub releases, after asking you.
- **Backup on every save**, a safety check that refuses to save while the game is
  running, and an **Advanced** tab that exposes every raw field in the hero and party data.

## Download

Grab the file for your system from the [Releases](https://github.com/jjanow/underkeeper/releases) page:

| System | File | How to run |
|---|---|---|
| Windows | `Underkeeper-windows-x64.exe` | Double-click. If SmartScreen complains, choose *More info → Run anyway*. |
| Linux / Steam Deck | `underkeeper-linux-x64.tar.gz` | Extract, then run `./underkeeper`. |
| macOS (Intel + Apple Silicon) | `Underkeeper-macos-universal.zip` | Unzip, then right-click `Underkeeper.app` → *Open* the first time (the app isn't notarized). Or run `xattr -dr com.apple.quarantine Underkeeper.app`. |

Version 0.3.0 and later update themselves (see [Updates](#updates)). Older
versions don't, so download 0.3.0 or later once by hand.

## Updates

At startup Underkeeper asks GitHub for the latest release of this repository.
Pre-releases and drafts are ignored. If a newer version exists, a bar appears under
the title with a **What's new** link. Click **Update and restart** to install it,
or **Later** to hide the bar until next time.

- The update is downloaded for your system and checked against the SHA-256
  checksum GitHub publishes for that download. If the checksum doesn't match or
  is missing, nothing is changed.
- The new version replaces the program in place: the `.exe` on Windows, the
  `underkeeper` file on Linux, and `Underkeeper.app` on macOS. Then the app
  restarts into it. Your settings and backups stay where they are.
- The button is disabled while you have unsaved changes. Save or revert first.
- The version button at the top right (e.g. `v0.4.0`) has **Check for updates
  now**, a **Check for updates at startup** toggle, and a link to the Releases page.
- If the check fails at startup (offline, GitHub unavailable), you won't see an
  error. A manual check tells you what went wrong.
- Updating needs write access to the folder the program is in. If the update
  can't be installed, the bar says why and links to the Releases page so you can
  download it yourself.
- **macOS:** move `Underkeeper.app` to your Applications folder (or any folder of your
  own) before updating. If you run it straight from Downloads without moving it,
  macOS runs a temporary read-only copy that can't be updated.

## How to use

1. **Quit Underkeep to the desktop.** (The editor won't save while the game is running.)
2. Open Underkeeper. Your save folder and saves show up on the left.
3. Pick a save, pick a hero, change what you want.
4. Click **Save changes**. The original file is backed up first. Use the
   **Backups** button to find the backups.
5. Start the game from Steam or GOG Galaxy and load that save.

### Steam Cloud and GOG Galaxy cloud saves

Underkeep keeps its saves in Steam Cloud (or GOG Galaxy's cloud saves for the GOG
version). When you edit a save while the game is closed, the launcher notices the
change at the next launch and uploads your edited file. If Steam shows a
**Cloud Conflict** dialog, or GOG Galaxy asks which saves to keep, choose the
**local** files (the ones on this computer), or your edits will be replaced by the
cloud copy.

## Where the saves are

The editor looks in all of these on its own. Use **Choose folder…** if yours is somewhere else.

| Setup | Folder |
|---|---|
| Windows | `C:\Program Files (x86)\Steam\userdata\<your id>\2411200\remote` |
| Windows, GOG | `%LocalAppData%\GOG.com\Galaxy\Applications\<game id>\Storage\Shared\Files` (the game id is a long number, e.g. `60010286278343253`) |
| Linux / Steam Deck (incl. Proton) | `~/.steam/steam/userdata/<your id>/2411200/remote` |
| Linux, Flatpak Steam | `~/.var/app/com.valvesoftware.Steam/.local/share/Steam/userdata/<your id>/2411200/remote` |
| macOS (CrossOver) | `~/Library/Application Support/CrossOver/Bottles/<bottle>/drive_c/Program Files (x86)/Steam/userdata/<your id>/2411200/remote` |
| macOS (Whisky) | `~/Library/Containers/com.isaacmarovitz.Whisky/Bottles/<bottle>/drive_c/Program Files (x86)/Steam/userdata/<your id>/2411200/remote` |
| GOG in a Wine bottle (CrossOver, Whisky, Heroic, Bottles) | `<bottle>/drive_c/users/<user>/AppData/Local/GOG.com/Galaxy/Applications/<game id>/Storage/Shared/Files` |

To paste the GOG folder into Windows Explorer's address bar, use
`%LocalAppData%\GOG.com\Galaxy\Applications` and open the numbered folder
that has `Storage\Shared\Files` with `save_…` files in it.

The game install (used for item names and the **Gear requirements** view) is
also found for both versions: Steam libraries, GOG Galaxy's registry entries,
`C:\Program Files (x86)\GOG Galaxy\Games\Underkeep` and Heroic's
`~/Games/Heroic/Underkeep`. If it isn't found, use **Set game folder…**.

Files are named `save_1`, `save_2`, … plus `save_auto` and `save_quick`.

## What you can edit

| Tab | Fields |
|---|---|
| Stats | Name, class, race, Strength, Dexterity, Endurance, Wisdom, level, experience, skill points, current health / mana / stamina / zeal, party coins |
| Skills & traits | Every skill level (up to the game's max), add missing class skills, add or remove racial traits and mutations |
| Gear requirements | All items with requirements, highlighted for the selected hero. *Meet every stat requirement* raises each attribute to the highest value any item needs. |
| Advanced | Every raw value stored for the hero and the party |

Notes:

- Attributes are the hero's **base** values. Gear such as a Ring of Wisdom adds on top in game.
- Maximum health, mana and stamina are calculated by the game from class, level
  and attributes, so they aren't stored in the save. Raising Endurance or Wisdom raises them.
- **Changing class** works like re-rolling the hero as the new class, and shows
  what will change before applying it:
  - The skill list becomes the new class's skills. You pick the starting skill
    (for a Voidcaller, the active aura). Free class skills are added (a Paladin's
    Stun Undead, a Voidcaller's three auras).
  - Every skill level earned beyond the starting ones is refunded as skill points.
  - The new class's main attribute is raised to its starting minimum if it's
    lower (e.g. a new Thief gets at least 8 DEX).
  - Equipped gear locked to another class goes to the party inventory.
  - Optionally, the new class's starting items are added to the inventory.
  - Class-only state (Paladin zeal, Voidcaller aura) is reset.

## Building from source

Requires [Rust](https://rustup.rs) (stable).

```sh
cargo build --release
./target/release/underkeeper
```

`cargo test` checks that the save codec reproduces every game data file
byte-for-byte. It finds your Underkeep install automatically, or set
`UNDERKEEP_DIR`. Set `UNDERKEEP_SAVES` to a folder of save files to also test
editing them.

### Releasing

Bump `version` in `Cargo.toml`, commit, and push the commit together with a tag
`vX.Y.Z` that matches it (`git push origin main vX.Y.Z`). Pushes to `main` don't
start CI. It runs only for tags, pull requests and manual runs, so a release is
tested and built once. CI builds all three platforms and publishes a GitHub release with
`Underkeeper-windows-x64.exe`, `underkeeper-linux-x64.tar.gz` and
`Underkeeper-macos-universal.zip`. The release job fails if the tag and
`Cargo.toml` disagree. Without that check, the new build would report the old
version and keep offering itself as an update. The in-app updater finds these
exact file names, so keep them as they are.

To test the updater, set `UNDERKEEPER_FAKE_VERSION=0.1.0` to make the app treat
itself as an old version. Debug builds only check for updates at startup when
this is set. `cargo test -- --ignored live_update` downloads, verifies and
installs the latest real release into a temporary folder.

### Save format

Saves and game data use the same small binary container ("DfT"): a string table
followed by a typed key/value tree. `src/dft.rs` documents the layout.

## Disclaimer

This is a fan-made tool, not affiliated with or endorsed by the developers of
Underkeep. Keep the automatic backups until you've confirmed an edited save loads.
No game files are included in this repository; names and item data are read from
your own installation at runtime.
