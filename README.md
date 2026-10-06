# OpenThunder

A small **War Thunder "Air Realistic Battle"-style** flight prototype built with
[Bevy](https://bevy.org) 0.19.

Right now it contains only what is needed to *fly around a simple map*:

- A physical 6-DoF flight model with lift, drag, stalls, air density and
  weathervane stability.
- A per-section damage model (hit points for wings, engine, tail, fuselage) that
  feeds back into the flight model.
- War Thunder Air-RB-style controls (mouse-aim instructor + keyboard).
- Three aircraft, each tuned to its War Thunder data sheet: the **F4U-4 Corsair**,
  the **Bf 109 G-6** and the **Spitfire F Mk IXc**. Pick one in the launcher.
  New aircraft are pure data — see below.
- **Multiplayer** on a dedicated server (see the companion
  [openthunder_server](https://github.com/Wynplusplus/openthunder_server) repo):
  pick a server in the launcher and fly with others.
- **Combat**: projectile guns with ballistic tracers, swept hit detection, and a
  per-section damage model.

There are no AI opponents or match objectives yet.

---

## Build & run

```sh
cd openthunder
cargo run                 # the game (debug; deps are optimized, see Cargo.toml)
cargo run --release       # the game, optimized
cargo run --bin launcher  # the TUI launcher
```

> **Linux note:** the default Bevy gamepad backend (`gilrs`) needs the `libudev`
> development package. This project uses an explicit Bevy feature list that
> excludes gamepads, so it builds without it. Add the `gamepad` feature back in
> `Cargo.toml` if you want controller support (and install `libudev-dev`).

---

## Launcher

`cargo run --bin launcher` opens a small terminal UI (built with plain ANSI
escapes, no extra TUI dependency) where you can:

- **Launch game** — starts the game (with the chosen aircraft and server) and
  returns to the launcher when it exits.
- **Select aircraft** — choose between the F4U-4 Corsair, Bf 109 G-6 and
  Spitfire F Mk IXc.
- **Server** — single-player, or one of the servers in `servers.conf`.
- **Display** — switch between **Fullscreen** (default) and Windowed.
- **Edit keybinds** — pick an action, press a new key, and it is saved instantly.
- **Reset keybinds to defaults**.

Settings are stored in shared config files:

```
$XDG_CONFIG_HOME/openthunder/keybinds.conf     # keybinds
$XDG_CONFIG_HOME/openthunder/settings.conf     # aircraft, server, display, name
$XDG_CONFIG_HOME/openthunder/servers.conf      # server list (name = host:port)
# or ~/.config/openthunder/...
```

They are plain `name = value` text files, so you can edit them by hand too. The
game loads them at startup (and creates them with defaults if missing). You can
also override things on the command line, e.g.
`cargo run -- --plane "Bf 109 G-6" --server 127.0.0.1:7777`.

---

## Multiplayer

OpenThunder can join a dedicated server (a separate repo,
[openthunder_server](https://github.com/Wynplusplus/openthunder_server)). The
server keeps the player roster and broadcasts snapshots at 20 Hz; the client
streams its own aircraft state and renders everyone else.

1. Start a server: `cargo run --release` in `openthunder_server` (listens on
   `0.0.0.0:7777`).
2. In the launcher choose **Server → Local**, or run the game with
   `--server 127.0.0.1:7777`.
3. Fly together. Add your own servers in `servers.conf`.

Maps and gamemodes live on the **server**: a map file picks a gamemode and
configures its rules, so both are easy to extend. See the server repo's README.

### Team Deathmatch

The `team_deathmatch` gamemode splits players into two teams (Blue / Red) and
races to a kill target. The server credits a kill to the last attacker when a
client reports its own destruction, and restarts the round when a team reaches
`score_limit` or `time_limit` runs out. The client shows a scoreboard (team
scores, round timer) at the bottom of the screen, a **kill feed** top-right, and
**respawns you automatically** a few seconds after you are shot down.

The `pacific_islands.map` map runs it over an island chain: the client builds
matching island terrain (water, beaches, runways, trees) from the server's map
name, and switches to it when you connect.

### Spotting

Air RB-style spotting keeps the sky from being a wall of nameplates:

- Aircraft are only **drawn within 9 km** (`render_distance`); beyond that they
  vanish from view.
- An **enemy** gets a red marker only when *spotted*: within 7 km inside the
  view cone (WT's "Keen Vision"), or always within 1.5 km all-round
  ("Awareness").
- **Friendlies** get a blue marker within 7 km; without teams everyone is
  neutral (grey).

Ranges live in `spotting.rs` and a server can tune them through its crew config
(`render_distance`, `detection_range`, `awareness_range`, `view_cone_deg`).

The server also ships its **pilot crew configuration** (`crew.conf`) to every
client on connect, so a server operator can tune the pilot's g-tolerance for the
whole server — see [Pilot g-tolerance and blackout](#pilot-g-tolerance-and-blackout).

The HUD shows the connection status (single-player / connecting / online / map +
gamemode) and the server's pilot tolerance.

---

## Controls (Air RB style)

Defaults — all of these can be changed in the launcher:

| Input            | Action                                   |
| ---------------- | ---------------------------------------- |
| Mouse            | Aim: the nose follows the pointer (War Thunder mouse aim) |
| `Up` / `Down`    | Pitch up / down (manual override)        |
| `Ctrl` / `Shift` | Pitch up / down (secondary bindings)     |
| `A` / `D`        | Roll left / right (manual override)      |
| `Q` / `E`        | Yaw left / right (manual override)       |
| `W` / `S`        | Throttle up / down (hold up at 100% for WEP) |
| `F` / `V`        | Flaps down / up (combat → takeoff → landing) |
| `B`              | War emergency power (WEP, overheats)     |
| `G`              | Landing gear down / up                   |
| `R`              | Respawn in the air                       |
| `1` / `2` / `3`  | Apply test damage: left wing / engine / tail |
| `0`              | Repair everything                        |
| `C`              | Free look (hold to orbit the camera)     |
| `RMB`            | Zoom in / out (toggle)                   |
| `Space` / LMB    | Fire guns                                |
| `Esc`            | In-game menu (Resume / Quit to Desktop)  |

Move the mouse once to "engage" mouse aim. From then on the aircraft points
wherever the cursor is: the instructor banks into the turn and pulls so the nose
follows the pointer. Move the cursor off-centre to turn, and bring it back to the
centre to level off and fly straight. The keyboard pitch/roll/rudder keys remain
available as manual overrides, and `A`/`D` always give full roll authority.

The **camera follows where you are aiming**: for small cursor movements it stays
behind the nose, and once the cursor nears the **edge of the screen** the view
orbits with it (WT: "the camera accelerates toward the cursor"). It stays **level
with the horizon** rather than rolling with the aircraft, like War Thunder mouse
aim, and the aircraft stays in view. Like War Thunder it also **pulls back when
you accelerate or brake hard** (it is the acceleration, not the speed, that moves
it) so you can read your energy state at a glance. **Zooming in locks the view
behind the nose** — the orbit fades out as the zoom eases in, as in WT.

Press **`Esc`** at any time for the in-game menu (Resume / Quit to Desktop). The
world **keeps flying** while it is open — the simulation is never paused; the
aircraft just continues with neutral controls. Use `Up`/`Down` and `Enter`.

**Hold `C`** to free-look: the mouse then orbits the camera around the aircraft
(War Thunder style) while the aircraft keeps flying. Release to ease back behind
the nose. The mouse does not steer the aircraft while free-looking.

When the game starts you are on the **spawn screen**: choose an aircraft with
`Up`/`Down` and press `Enter` to spawn into the air. The list is the built-in
planes, or the planes the server sent if you are connected. The in-game menu
(`Esc`) has **Change plane** to come back to it.

### Instructor (mouse aim)

War Thunder's instructor is a set of programs that turn mouse movements into
control-surface deflections while keeping the aircraft inside its realistic
limits (see the WT article
[*How the Instructor Works*](https://warthunder.com/en/news/4366-wiki-article-how-the-instructor-works-en/)).
In mouse-aim it does all of the following, and so do we:

- **Prevents the wing from reaching the critical angle of attack**, preserving
  lift — you cannot stall by yanking the mouse.
- **Trims the aircraft to hold its current flight trajectory**: release the
  mouse and it keeps flying the way it was, instead of pitching as the speed
  changes. This is why you can throttle down without the nose wandering, and it
  trims a banked turn for the extra lift it needs.
- **Compensates for the propeller's reaction torque** (and the airframe's
  aerodynamic asymmetry) with a small counter-roll.
- **Slows the g buildup** as it approaches the airframe's structural limit.
- Points the nose where the cursor is: it **banks into the turn and pulls**, then
  levels the wings as the target comes onto the nose. Pressing a **manual** key
  on an axis **overrides** the instructor on that axis (pitch / roll / yaw), but
  the instructor still keeps the aircraft inside its limits. While the reticle is
  right on the crosshair (within a degree) and you steer manually, it **rides
  along with the aircraft** (WT behaviour), so releasing the keys holds the new
  heading instead of snapping back to the old aim.

The mouse moves a **world-space aim direction**, not a screen position, so the
on-screen cursor **drifts back toward the gun crosshair** as the nose catches up
— exactly like War Thunder. The **crosshair itself always marks the gun
direction** (the nose projected on screen), so it leaves the screen centre as the
camera orbits. The aim cursor is **kept on screen**: if a zoomed-in view would
push it past the edge, the cursor is clamped to the edge and the aim is pulled in
with it. The OS cursor is hidden and locked while flying (and released while the
menu is open), so the mouse is relative and never gets stuck at the window edge.

WT's control modes differ in how much the instructor does — *mouse aim* (full
control), *simplified* (stall protection + trim), *realistic* (trim only) and
*full* (instructor off). We model the full mouse-aim behaviour, in
`read_player_input` / `aim_controls` in `flight.rs`.

---

## Flight model

Implemented in `src/flight.rs`. It is a simplified but genuinely physical 6-DoF
model, tuned to the F4U-4's real data sheet. Each physics step:

1. Compute angle of attack / sideslip from the velocity in the body frame.
2. Lift from a real lift-curve slope that **stalls** past the critical AoA; drag
   is parasitic + induced (`CD = CD0 + CL² / (π · e · AR)`) plus a **transonic
   drag rise** near the Mach limit.
3. Thrust from a **propeller power model** (`T = η·P/V`, capped by static
   thrust). Power is constant up to the engine's **critical altitude**, then
   falls off — so the aircraft is fastest high up, exactly like the real thing.
4. Sum lift + drag + thrust + gravity + sideslip side-force, integrate velocity
   and position.
5. Drive body angular rates from the controls, with **control authority** that
   grows with airspeed, goes mushy when slow, and **stiffens at high indicated
   airspeed / Mach** (the classic "the controls lock up in a dive"). Add
   **weathervane stability** and rate damping, then integrate the orientation as
   a quaternion.

Air density falls exponentially with altitude, and **indicated airspeed** (used
for the limits and stiffening) is derived from it.

### Air RB features

- **Stall** with a post-stall lift drop.
- **Flaps** in combat / takeoff / landing positions, adding lift and drag, with
  speed limits and instructor **auto-retract** when overspeed.
- **War emergency power (WEP)**: an extra power setting on engines that had it
  (the Corsair's water injection, the Spitfire's +25 lb boost — but not the
  Bf 109 G-6's DB 605A). At full throttle, keep holding the **throttle-up** key
  or hold the **WEP** key (`B`) to push past 100% — WT's "110%" notch. It builds
  heat and cuts out if held too long, and must cool before re-engaging. The HUD
  shows `WEP` / `WEP ready` / `WEP needs 100% throttle`.
- **Propeller torque** roll, trimmed out by the instructor.
- **Structural limits**: each aircraft has its own g limit and IAS redline;
  exceeding them damages the airframe.
- **Control stiffening / compressibility** at high speed.
- **Landing gear** (toggle with `G`): retractable wheels with drag when extended.
- **Zoom** (tap the right mouse button to toggle): narrows the field of view
  WT-style, pulls the camera in and **lowers the mouse sensitivity** to match, so
  aiming stays precise at long range. While zoomed the camera **stops orbiting**
  and locks behind the nose, and the **aim cursor stays on screen** (clamped to
  the edge, pulling the aim in with it). Any action can be bound to a mouse
  button (LMB/RMB/MMB) as well as a key.
- **Landing and take-off**: you spawn on the runway, throttle up, rotate and
  climb away; come back with the gear down, touch down gently (a hard descent —
  or a belly landing with the gear up — damages the airframe), roll out and stop.
  Steering on the ground is with the rudder.
- **G-limiter**: the instructor eases off as the wing approaches its structural
  g limit, so mouse aim alone will not normally rip the airframe.
- **Pilot g-tolerance**: a trained pilot holds ~6.5 g; harder or longer pulls
  tunnel the vision down to a blackout (or a red-out when pushing), with stamina
  and recovery, exactly like Air RB.

### Pilot g-tolerance and blackout

Implemented in `src/pilot.rs`. The pilot is modelled separately from the
airframe, as in War Thunder:

- A trained crew tolerates about **6.5 g** (WT's maxed "G-tolerance" is ~6.9 g)
  and **−3 g**. Pull past that and a **blackout veil** closes in from the edges;
  push past it and you **red out** instead.
- **Stamina** drains while you manoeuvre and recovers in level flight. A tired
  pilot tolerates about 30% less g, so a long fight wears you down.
- At full blackout the pilot is **unconscious and loses control** until the
  aircraft unloads and vision returns.
- The **structural g limit is higher than the pilot's tolerance**, so in a hard
  turn you black out well before the wings are at risk — just like Air RB.

The overlay is a procedurally generated radial gradient (tunnel vision) tinted
black for blackout or red for red-out.

**Configurable per server.** The dedicated server ships a `crew.conf` (shipped
in the `CREW` protocol message on connect) that overrides these values for every
client on that server — lower `g_tolerance` for a harsher, more realistic server,
raise it for a forgiving one. Single-player uses the built-in defaults. Keys:
`g_tolerance`, `negative_g_tolerance`, `blackout_rate`, `recovery_rate`,
`stamina_drain`, `stamina_recovery` (unknown keys are ignored).


## Aircraft

All three are tuned to their War Thunder (RB) data sheets.

| | F4U-4 Corsair | Bf 109 G-6 | Spitfire F Mk IXc |
| --- | --- | --- | --- |
| Nation | USA | Germany | Great Britain |
| Engine | R-2800-18W | DB 605A | Merlin-61 |
| WEP | water injection (+20%) | none | +25 lb boost (+25%) |
| Max speed | 711 km/h @ 9,000 m | 669 km/h @ 5,500 m | 642 km/h @ 8,537 m |
| Rate of climb | 18.5 m/s | 19.6 m/s | 18.9 m/s |
| Turn time | 20.0 s | 20.0 s | 17.2 s |
| Wing loading | 190 kg/m² | 198 kg/m² | 152 kg/m² |
| G limit | +11 / −4 | +13 / −6 | +10 / −5 |
| IAS redline | 885 km/h | 790 km/h | 774 km/h |
| Flaps (C/T/L) | 388/299/253 | 438/409/260 | 260 km/h |

Each has its own mass, wing area/span, power, lift/drag coefficients, control
rates, stability and damping, so they fly differently: the Corsair is fast and
rolls hard, the Bf 109 climbs and zooms but stiffens up at speed, and the
Spitfire turns and climbs best at low speed.

## Combat

Implemented in `src/combat.rs`.

- Each aircraft has data-driven **guns** (`GunSpec`: caliber, rate of fire,
  muzzle velocity, damage, spread, muzzle positions, ammo) — 6× .50 cal for the
  Corsair, a 20 mm + 2× 13 mm for the Bf 109, 4× 20 mm Hispanos for the Spitfire.
- Fire with the **left mouse button** or **`Space`**. Rounds are **ballistic
  projectiles**: they inherit the aircraft's velocity plus muzzle velocity, then
  fall under gravity and lose speed to drag, drawn as glowing tracers.
- Hit detection is **swept** (segment vs. oriented box) so fast rounds cannot
  tunnel through a target.
- A hit is classified to a section by where it landed (wing / engine / tail /
  fuselage) and damages that section's `DamageModel`, which the flight model then
  reads.
- A destroyed fuselage finishes the aircraft: no thrust, no control, heavy drag.
- The HUD shows remaining ammo, the armament and hit markers.
- A gun **crosshair** always marks **where the guns are pointing**: the nose is
  projected on screen each frame, so it tracks the real firing direction (and the
  camera orbit) rather than sitting fixed at screen centre.

### Test targets

In **free flight** (single-player or a `free_flight` server) a set of shootable
targets is placed near the runway, War Thunder test-flight style:

- **Six tanks** and **three fuel tanks** on the ground, plus
- **Three target planes** circling fixed orbits — they **do not fight back**.

Each has a hit box and health, blows up when destroyed (planes tumble down
first), and **respawns after 20 s** so you can keep practising. All of them get
an **Air RB-style marker** (`Tank 1.2 km`, `Target plane 0.8 km`) once spotted,
and the HUD shows `Targets N/12 destroyed`. In team modes the targets are
cleared away.

Implemented in `src/targets.rs`; the hit detection in `combat.rs` uses each
target's own `HitBox` (or the aircraft default for the planes).

## Damage model

Implemented in `src/damage.rs`. Every aircraft owns a `DamageModel` with one
`PartState` per `AircraftPart`. The flight model reads the integrity of each
part:

- **Wings** → less lift when damaged.
- **Engine** → less thrust when damaged.
- **Tail** → weaker stability and sluggish rotation when damaged.

Real weapons just need to call `DamageModel::apply_damage(part, amount)`.

---

## Adding a new aircraft

A plane is **data**. The built-in planes are defined in
`openthunder::plane_config` (`src/plane_config.rs`), using the same `key = value`
+ `[gun N]` format the server uses.

- **On a server**, a plane is just `planes/<id>/plane.conf`. The client downloads
  every plane the server has when it connects, so **new server planes need no
  client rebuild**. See the server repo's README.
- **Built-in / single-player**: edit `default_planes()` in `src/plane_config.rs`
  (and keep the server's matching `plane.conf` in sync). The launcher's list
  lives in `src/planes.rs`; a unit test keeps the two in sync.

The visual model is generated from the plane's model parameters (length, wing
chord, tail span, colour), so there is no per-plane rendering code to write.

## Project layout

```
src/
  main.rs            game entry point: app setup, plugins, window, sky/ambient
  lib.rs             shared library (used by the game and the launcher)
  keybinds.rs        keybind config: defaults, load/save, supported keys
  planes.rs          the list of aircraft offered by the launcher
  plane_config.rs    plane definitions (flight model + model + guns)
  servers.rs         the server list shown by the launcher
  settings.rs        player settings (aircraft, server, display, name)
  protocol.rs        wire protocol (shared verbatim with the server repo)
  bin/launcher.rs    the TUI launcher (raw ANSI + libc termios)
  aircraft.rs        AircraftSpec (built from plane configs), registry, spawning
  flight.rs          keybind resource, controls (mouse instructor + keyboard), flight model
  net.rs             multiplayer client (background threads + remote aircraft)
  camera.rs          third-person chase camera + distance fog
  world.rs           ground, runway, scattered landmarks, sun
  damage.rs          per-section damage model + debug damage keys
  hud.rs             telemetry overlay
  menu.rs            in-game menu (Esc); the world keeps running while open
  spawn_menu.rs      pre-spawn plane selection screen
  crosshair.rs       gun crosshair + the mouse-aim cursor (and cursor capture)
  pilot.rs           crew g-tolerance, blackout/redout, tunnel-vision overlay
  match_client.rs    team deathmatch state: scoreboard, kill feed, respawn
  spotting.rs        Air RB-style spotting: render culling + aircraft markers
  targets.rs         free-flight test targets (shootable, respawning)
```

## Known simplifications

- Lift acts along the body up axis (not exactly perpendicular to the airflow),
  which is fine at normal angles of attack but not at extreme ones.
- Terrain is flat (training) or a single rounded island (islands); the ground
  model uses an approximate height, and only the home island is landable.
- Aircraft always sit level on the ground (no taildragger attitude) and there is
  no wheel-level collision, just a landing surface.
- The pilot model uses a fixed crew-skill level; there is no crew progression.
- No audio.

---

## Legal / disclaimer

This is an **unofficial, non-commercial fan prototype**. It is **not affiliated
with, endorsed by, or connected to Gaijin Entertainment**.

"War Thunder" is a trademark of Gaijin Entertainment. It is referenced here only
descriptively, to say what kind of flight model and control scheme this project
is inspired by.

- **No assets** (models, textures, sounds, data files) from War Thunder or any
  other game are included. Every mesh is generated procedurally from Bevy
  primitives.
- All code in this repository is original.
- Aircraft performance figures are public, historical facts about real aircraft.
- Bevy is used under its MIT / Apache-2.0 license.

If you are a rights holder and have a concern, please open an issue.

## License

This project is licensed under the **GNU General Public License v3.0 or later**
(see [`LICENSE`](LICENSE)). Bevy is used under its MIT / Apache-2.0 license.
