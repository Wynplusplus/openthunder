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

There is **no combat yet**.

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

- **Launch game** — starts the game (with the chosen aircraft) and returns to the
  launcher when it exits.
- **Select aircraft** — choose between the F4U-4 Corsair, Bf 109 G-6 and
  Spitfire F Mk IXc.
- **Edit keybinds** — pick an action, press a new key, and it is saved instantly.
- **Reset keybinds to defaults**.

Settings are stored in shared config files:

```
$XDG_CONFIG_HOME/openthunder/keybinds.conf     # keybinds
$XDG_CONFIG_HOME/openthunder/settings.conf     # selected aircraft
# or ~/.config/openthunder/...
```

They are plain `name = value` text files, so you can edit them by hand too. The
game loads them at startup (and creates them with defaults if missing). You can
also override the aircraft on the command line: `cargo run -- --plane "Bf 109 G-6"`.

---

## Controls (Air RB style)

Defaults — all of these can be changed in the launcher:

| Input            | Action                                   |
| ---------------- | ---------------------------------------- |
| Mouse            | Aim: the nose follows the pointer (War Thunder mouse aim) |
| `Up` / `Down`    | Pitch up / down (manual override)        |
| `Ctrl` / `Shift` | Pitch up / down (secondary bindings)     |
| `A` / `D`        | Roll left / right (manual override)      |
| `Q` / `E`        | Yaw left / right (rudder)                |
| `W` / `S`        | Throttle up / down                       |
| `F` / `V`        | Flaps down / up (combat → takeoff → landing) |
| `B`              | War emergency power (WEP, overheats)     |
| `R`              | Respawn in the air                       |
| `1` / `2` / `3`  | Apply test damage: left wing / engine / tail |
| `0`              | Repair everything                        |

Move the mouse once to "engage" mouse aim. From then on the aircraft points
wherever the cursor is: the instructor banks into the turn and pulls so the nose
follows the pointer. Move the cursor off-centre to turn, and bring it back to the
centre to level off and fly straight. The keyboard pitch/roll/rudder keys remain
available as manual overrides, and `A`/`D` always give full roll authority.

### Instructor (mouse aim)

Like War Thunder's instructor, the mouse-aim system does more than point the
nose. It:

- banks and pulls so the nose follows the pointer, and **levels the wings** when
  the pointer is centred;
- keeps the wing **off the critical angle of attack** (stall protection) and
  **eases off near the structural g limit**;
- **trims out the propeller torque** with a small counter-roll;
- lets you override any axis from the keyboard while it keeps the aircraft
  inside its limits.

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
- **War emergency power (WEP)**: extra thrust that builds heat and cuts out if
  held too long.
- **Propeller torque** roll, trimmed out by the instructor.
- **Structural limits**: each aircraft has its own g limit and IAS redline;
  exceeding them damages the airframe.
- **Control stiffening / compressibility** at high speed.

## Aircraft

All three are tuned to their War Thunder (RB) data sheets.

| | F4U-4 Corsair | Bf 109 G-6 | Spitfire F Mk IXc |
| --- | --- | --- | --- |
| Nation | USA | Germany | Great Britain |
| Engine | R-2800-18W | DB-605AM | Merlin-61 |
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

The whole point of the structure is that a new plane is *just data*.

1. Add a function in `src/aircraft.rs` returning an `AircraftSpec`
   (copy `bf109_g6()` and change the numbers).
2. Register it in `AircraftPlugin`:

   ```rust
   specs: vec![f4u_4_corsair(), bf109_g6(), spitfire_mk9(), my_new_plane()],
   ```
3. Add it to `openthunder::planes::PLANES` (in `src/planes.rs`) so the launcher
   can offer it. A unit test fails if the two lists drift apart.

The visual model is generated from the spec (wing span, colour, …), so there is
no per-plane rendering code to write.

## Project layout

```
src/
  main.rs            game entry point: app setup, plugins, window, sky/ambient
  lib.rs             shared library (used by the game and the launcher)
  keybinds.rs        keybind config: defaults, load/save, supported keys
  planes.rs          the list of aircraft offered by the launcher
  settings.rs        player settings (selected aircraft)
  bin/launcher.rs    the TUI launcher (raw ANSI + libc termios)
  aircraft.rs        AircraftSpec + specs, registry, runtime state, spawning
  flight.rs          keybind resource, controls (mouse instructor + keyboard), flight model
  camera.rs          third-person chase camera + distance fog
  world.rs           ground, runway, scattered landmarks, sun
  damage.rs          per-section damage model + debug damage keys
  hud.rs             telemetry overlay
```

## Known simplifications

- Lift acts along the body up axis (not exactly perpendicular to the airflow),
  which is fine at normal angles of attack but not at extreme ones.
- Ground interaction is a soft clamp; there is no proper landing/crash.
- No combat, no multiplayer, no audio.

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
