# FPStick Recorder

A small Windows program that records an Overwatch match and packs the frame and network timing for a run
page. It chooses, records, packs and sends. It draws no charts; the run page opens in the browser.

It is the recording half of FPStick (fpstick.com): FPS and tick, measured in a real match. The half that
judges a recording and draws the run page is not in this repository.

Status: beta. It records frame rate performance and, if switched on, game network stats. Inputs are not
recorded. There is no public run-page server yet, so for now this repository is for reading and building:
"Send" goes to the address typed into "Send to" on the last screen (a server on your own network, over
plain http), or, left empty, to a run-page program on the same PC.

Not affiliated with or endorsed by Blizzard Entertainment or Intel. Running software beside a game is your
own decision under that game's terms; the sections below say exactly what this program reads.

## What it does when you press Record

1. Asks Windows for administrator rights, once. PresentMon cannot record without them.
2. Counts down five seconds, so you can get back into the game.
3. Starts Intel's PresentMon, unchanged, with the command the window shows. A sound says it has started.
4. Stops when the length you picked is over, when you press (or hold) your stop key, after 30 minutes,
   or when the game closes. A second sound says it has stopped.
5. Reads the file PresentMon wrote and packs the frame timing into `frames.fpsf` beside it.
6. Shows what would be sent and what stays on the PC. Nothing is sent before you press Send.

The stop key is read by asking Windows for the key's state a few dozen times a second. The recorder does
not install a hook into the mouse's or keyboard's input.

## Game network stats

- **Packets.** Windows keeps its own record of every UDP packet a program sends or receives. The recorder
  listens to it while recording and keeps, for the game's process only, each packet's time, direction and
  size. It never sees what is in a packet. The times are on the same clock PresentMon stamps frames with.
- **Route.** Once the game is talking to a server, the way there is looked up once, and three routers on it
  (the home router, the provider's first, and the last before the server) are pinged five times a second.
- **Addresses** are used while recording, to tell the server's packets from others and to aim the pings.
  They are never written to a file.

## What is in a recording's folder (Documents\Recorder\<date and time>)

- `frames.csv`: PresentMon's own file. It stays on the PC.
- `frames.fpsf`: the packed frame timing. This is what Send sends.
- `udp.csv`, `ping.csv`, `net.json`: the network stats, if switched on. Remote ends are numbered and
  routers are named by role; there are no addresses in them. Send packs the match server's rows of
  `udp.csv` and all of `ping.csv` (format FPSN1, described at the top of `framedata/src/net.rs`) and
  sends that with the frames: when each packet passed, which way, how large; when each ping was sent,
  to which router by role, how long the reply took. Packets to other remote ends stay on the PC.
- `run.json`: when it was recorded, the command that ran, what you typed in, the CPU model, graphics
  card model and screen mode read from the PC, and the graphics settings from the game's own settings
  file (`Documents\Overwatch\Settings\Settings_v0.ini`, the sections named "Render"), read when the
  recording starts and again when it ends. The settings stay on the PC for now.

## Building

Rust (stable) is all it needs: `cargo build --release` in this folder. The tests run anywhere:
`cargo test`, and `cargo test --manifest-path framedata/Cargo.toml` for the reader and packer.

`framedata` is the part that reads PresentMon's file and packs it (format FPSF1, described at the top of
`framedata/src/pack.rs`; the network timing is format FPSN1, in `framedata/src/net.rs`). The server that
judges a recording uses the same crate to unpack it, so the two agree on the formats by construction.

Other commands, for checking: `recorder info`, `recorder pack IN.csv OUT.fpsf`, and `recorder nettest`,
which sends five packets to this PC itself and checks that the network record reports each one exactly.

## Licence

MIT or Apache-2.0, at your choice: see `LICENSE-MIT` and `LICENSE-APACHE`.
PresentMon is Intel's, under the MIT licence. It is not part of this repository and is not shipped with the
recorder: the recorder starts the PresentMon that is installed on the PC.

## What it costs while recording

While the game is being recorded the recorder does only what the recording needs.

- **The window** is drawn once in a paused state and not again; no timer, no counters.
- **The recording thread** is asleep in Windows, waiting on PresentMon's process. It wakes 10 times a second to
  ask whether the stop key is down (40 times if a single press stops it). It runs below ordinary priority.
- **The stop key is asked for, not listened for.** A hook would sit in the path of every mouse and keyboard
  event; raw input would wake the recorder for every mouse report.
- **Game network stats:** Windows is asked for the four UDP events only, so other programs' traffic never
  reaches the recorder. The thread that reads them runs below ordinary priority; the events are stamped by
  Windows when they happen, so reading them late changes nothing. The ping threads keep ordinary priority,
  because a ping's time is taken on its thread. Nothing is written to disk until the recording has ended.
- Lists are sized for half an hour before the recording starts, so nothing is moved in memory during play.

Measured on the first PC (Ryzen 7 9850X3D) over 20 seconds of recording a test window, from Windows' count
of processor cycles (the command-line recorder, whose own loop adds 4 wake-ups a second the window does not have):

| | Processor time in 20 s | Wake-ups a second |
|---|---|---|
| Frames, stop key held for a second | 1.8 ms (0.009 % of one core) | 12 |
| Frames, stop key pressed once | 3.5 ms | 35 |
| Frames and network, two routers pinged | 21 to 24 ms (0.11 %) | 55 |

Nearly all of the network figure is the pings: five a second to each router. They are a tick box of their
own under "Game network stats" ("Ping the route to the server"); without them the game's packets are still recorded. Before these changes the same
measurement gave 7 to 9 ms and 43 wake-ups for frames, 31 to 33 ms and 85 wake-ups with network.

The system timer step stayed at 15.625 ms before, during and after: the recorder does not raise the timer
resolution. PresentMon's own cost is not in these figures, and none of this was measured with the game running.
