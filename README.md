# grelier

A desktop bar for Sway on Linux

## Overview

A the top level is a persistent window that says on the screen at all times, called the `bar`.  The `bar` lives either on the left or right side of the screen.  The bar contains a set of `panel`s.  There is a workspace `panel` that displays and allows navigation of desktop workspaces.  There is a `top_apps` panel that, can display a finite list of the top most used apps on the system.  There is a `gauge` panel which displays a set of configured gauges.  A gauge displays some information about the system or user state, and optionally can provide some basic management functions based on the `gauge`.  For example, the `wifi` gauge allows the user to select from the set of previously configured access points.  The `session` gauge displays system uptime and allows the user to suspend, reboot, or shutdown the system. The program can take configuration parameters from CLI arguments or use Xresource-style key-value pairs specified in `$HOME/.config/grelier/Settings-<version>.xresources`.

Generally right-click actions are read-only, for example seeing network download statistics, and left-click actions cause some change to occur, for example selecting the output audio device.

### Status

This project is in active development and should not be considered stable.

## Usage

```
Usage: grelier [-s <settings>] [--list-themes] [--list-gauges] [--list-panels] [-c <config>] [--list-settings] [--list-monitors] [--on-monitor <on-monitor>]

Workspace + gauges display

Options:
  -s, --settings    setting override; repeat for multiple pairs (key=value or key:value)
  --list-themes     list available themes and exit
  --list-gauges     list available gauges and exit
  --list-panels     list available panels and exit
  -c, --config      override the settings file path
  --list-settings   list app settings and exit
  --list-monitors   list available monitors and exit
  --on-monitor      limit bar to one monitor by name
  --help, help      display usage information
```

### Cargo features

- `workspaces` (default): enables the workspace panel.
- `top_apps` (default): enables the top apps panel.
- `gauges` (default): enables the gauges panel.

Examples:

```bash
# build with all panel features (default)
cargo build

# build without panel features
cargo build --no-default-features

# build with only the workspace panel
cargo build --no-default-features --features workspaces
```

## Multi-Monitor Support

By default, `grelier` opens a bar on all active monitors.

Use `--on-monitor <name>` to target exactly one monitor. Monitor names can be listed with:

```bash
grelier --list-monitors
```

## Configuration

Grelier reads from `$HOME/.config/grelier/Settings-<version>.xresources` on start for its configuration.  Use `--config` to override the settings file path.  Any configuration changes made interactively are immediately saved back to this file.  The file is regenerated each time, so any manual edits will be destroyed.  `grelier --list-settings` can be used to see all supported settings.  `grelier --list-gauges` will print all available gauges with descriptions.  `grelier --list-panels` will list the valid panel identifiers.

### Workspace styling

- `grelier.ws.corner_radius` (default `5.0`): Sets the roundness of workspace indicators.
- `grelier.ws.spacing` (default `2`): Controls the space between workspace indicators.
- `grelier.ws.transitions` (default `true`): Enables the focus/urgent transition animation.

### Gauge layout

- `grelier.gauge.spacing` (default `7`): Sets the vertical space between gauges.

### Bar Settings

| Setting | Default | Description |
| --- | --- | --- |
| `grelier.gauges` | `clock,date` | Comma-separated list of gauges to display. |
| `grelier.panels` | `workspaces,top_apps,gauges` | Comma-separated panel order. |
| `grelier.bar.orientation` | `left` | Bar placement on the screen. |
| `grelier.bar.theme` | `Nord` | Theme name to load. |
| `grelier.bar.theme.background` |  | Custom theme background color (RRGGBB or #RRGGBB). |
| `grelier.bar.theme.text` |  | Custom theme text color (RRGGBB or #RRGGBB). |
| `grelier.bar.theme.primary` |  | Custom theme primary color (RRGGBB or #RRGGBB). |
| `grelier.bar.theme.success` |  | Custom theme success color (RRGGBB or #RRGGBB). |
| `grelier.bar.theme.warning` |  | Custom theme warning color (RRGGBB or #RRGGBB). |
| `grelier.bar.theme.danger` |  | Custom theme danger color (RRGGBB or #RRGGBB). |
| `grelier.bar.width` | `28` | Bar width in columns. |
| `grelier.bar.border.blend` | `true` | Blend border colors with the bar background. |
| `grelier.bar.border.line_width` | `1.0` | Border line width. |
| `grelier.bar.border.column_width` | `3.0` | Border column width. |
| `grelier.bar.border.mix_1` | `0.2` | Border color mix level 1. |
| `grelier.bar.border.mix_2` | `0.6` | Border color mix level 2. |
| `grelier.bar.border.mix_3` | `1.0` | Border color mix level 3. |
| `grelier.bar.border.alpha_1` | `0.9` | Border alpha level 1. |
| `grelier.bar.border.alpha_2` | `0.7` | Border alpha level 2. |
| `grelier.bar.border.alpha_3` | `0.9` | Border alpha level 3. |

Example custom theme settings (Solarized Dark):
```xresources
grelier.bar.theme: Custom
grelier.bar.theme.background: #002B36
grelier.bar.theme.text: #839496
grelier.bar.theme.primary: #268BD2
grelier.bar.theme.success: #859900
grelier.bar.theme.warning: #B58900
grelier.bar.theme.danger: #DC322F
```

## Gauges

### `audio_in`
Input volume control with mute toggle and device menu. Monitors the default PulseAudio source volume and mute state.

| Setting | Default | Description |
| --- | --- | --- |
| `grelier.audio_in.step_percent` | `5` | Scroll step size for volume changes (percent). |

### `audio_out`
Output volume control with mute toggle and device menu. Monitors the default PulseAudio sink volume and mute state.

| Setting | Default | Description |
| --- | --- | --- |
| `grelier.audio_out.step_percent` | `5` | Scroll step size for volume changes (percent). |

### `battery`
Battery status and charging indicator. Monitors battery capacity and charging state from udev `power_supply`.

| Setting | Default | Description |
| --- | --- | --- |
| `grelier.battery.warning_percent` | `49` | Low-battery warning threshold (percent). |
| `grelier.battery.danger_percent` | `19` | Critical-battery threshold (percent). |

### `brightness`
Backlight brightness indicator with scroll-based adjustment. Monitors backlight brightness via `/sys/class/backlight`.

| Setting | Default | Description |
| --- | --- | --- |
| `grelier.brightness.step_percent` | `5` | Scroll step size for brightness changes (percent). |
| `grelier.brightness.refresh_interval_secs` | `2` | Refresh interval in seconds. |

### `clock`
Wall-clock time readout. Uses local system time (hour/minute, optional seconds).

| Setting | Default | Description |
| --- | --- | --- |
| `grelier.clock.showseconds` | `false` | Show seconds in the time display. |
| `grelier.clock.hourformat` | `24` | Hour format (`12` or `24`). |

### `cpu`
CPU utilization indicator with adaptive polling. Uses aggregate CPU usage from `/proc/stat`.

| Setting | Default | Description |
| --- | --- | --- |
| `grelier.cpu.quantitystyle` | `grid` | Quantity icon style. |
| `grelier.cpu.warning_threshold` | `0.75` | Warning threshold for usage. |
| `grelier.cpu.danger_threshold` | `0.90` | Danger threshold for usage. |
| `grelier.cpu.fast_threshold` | `0.50` | Usage level to switch to fast polling. |
| `grelier.cpu.calm_ticks` | `4` | Calm ticks before returning to slow polling. |
| `grelier.cpu.fast_interval_secs` | `1` | Fast polling interval in seconds. |
| `grelier.cpu.slow_interval_secs` | `4` | Slow polling interval in seconds. |

### `date`
Calendar date readout. Uses the local system date (month/day).

| Setting | Default | Description |
| --- | --- | --- |
| `grelier.date.month_format` | `%m` | `strftime` month format. |
| `grelier.date.day_format` | `%d` | `strftime` day format. |

### `disk`
Disk usage indicator for a filesystem path. Monitors used/total space for the configured path.

| Setting | Default | Description |
| --- | --- | --- |
| `grelier.disk.quantitystyle` | `grid` | Quantity icon style. |
| `grelier.disk.path` | `/` | Filesystem path to measure. |
| `grelier.disk.poll_interval_secs` | `60` | Poll interval in seconds. |
| `grelier.disk.warning_threshold` | `0.85` | Warning threshold for usage. |
| `grelier.disk.danger_threshold` | `0.95` | Danger threshold for usage. |

### `net_down`
Download throughput indicator. Monitors active interface receive rate from `/proc/net/dev`.

| Setting | Default | Description |
| --- | --- | --- |
| `grelier.net.idle_threshold_bps` | `10240` | Below this rate, show idle state. |
| `grelier.net.fast_interval_secs` | `1` | Fast polling interval in seconds. |
| `grelier.net.slow_interval_secs` | `3` | Slow polling interval in seconds. |
| `grelier.net.calm_ticks` | `4` | Calm ticks before returning to slow polling. |
| `grelier.net.iface_cache_ttl_secs` | `10` | Interface cache TTL in seconds. |
| `grelier.net.iface_ttl_secs` | `5` | Interface selection TTL in seconds. |
| `grelier.net.sampler_min_interval_ms` | `900` | Minimum sampler interval in milliseconds. |
| `grelier.net.sys_class_net_path` | `/sys/class/net` | Path to network interface sysfs. |
| `grelier.net.proc_net_route_path` | `/proc/net/route` | Path to routing table data. |
| `grelier.net.proc_net_dev_path` | `/proc/net/dev` | Path to interface counters. |

### `net_up`
Upload throughput indicator. Monitors active interface transmit rate from `/proc/net/dev`.

| Setting | Default | Description |
| --- | --- | --- |
| `grelier.net.idle_threshold_bps` | `10240` | Below this rate, show idle state. |
| `grelier.net.fast_interval_secs` | `1` | Fast polling interval in seconds. |
| `grelier.net.slow_interval_secs` | `3` | Slow polling interval in seconds. |
| `grelier.net.calm_ticks` | `4` | Calm ticks before returning to slow polling. |
| `grelier.net.iface_cache_ttl_secs` | `10` | Interface cache TTL in seconds. |
| `grelier.net.iface_ttl_secs` | `5` | Interface selection TTL in seconds. |
| `grelier.net.sampler_min_interval_ms` | `900` | Minimum sampler interval in milliseconds. |
| `grelier.net.sys_class_net_path` | `/sys/class/net` | Path to network interface sysfs. |
| `grelier.net.proc_net_route_path` | `/proc/net/route` | Path to routing table data. |
| `grelier.net.proc_net_dev_path` | `/proc/net/dev` | Path to interface counters. |

### `ram`
Memory utilization indicator with adaptive polling. Uses system RAM usage from `/proc/meminfo` (including shrinkable ZFS ARC).

| Setting | Default | Description |
| --- | --- | --- |
| `grelier.ram.quantitystyle` | `grid` | Quantity icon style. |
| `grelier.ram.warning_threshold` | `0.85` | Warning threshold for usage. |
| `grelier.ram.danger_threshold` | `0.95` | Danger threshold for usage. |
| `grelier.ram.fast_threshold` | `0.70` | Usage level to switch to fast polling. |
| `grelier.ram.calm_ticks` | `4` | Calm ticks before returning to slow polling. |
| `grelier.ram.fast_interval_secs` | `1` | Fast polling interval in seconds. |
| `grelier.ram.slow_interval_secs` | `4` | Slow polling interval in seconds. |

### `wifi`
Wi-Fi link status and signal indicator. Monitors connection state and link quality from `/sys/class/net` and `/proc/net/wireless`.

| Setting | Default | Description |
| --- | --- | --- |
| `grelier.wifi.quantitystyle` | `grid` | Quantity icon style. |
| `grelier.wifi.quality_max` | `70` | Maximum quality value used for scaling. |
| `grelier.wifi.poll_interval_secs` | `3` | Poll interval in seconds. |

### `test_gauge`
Internal gauge for cycling quantity icons and attention states. Uses synthetic values (no system monitoring).

| Setting | Default | Description |
| --- | --- | --- |
| `grelier.test_gauge.quantitystyle` | `pie` | Quantity icon style. |

## Build and Run

```shell
cargo build --release
./target/release/grelier &
cat ~/.config/grelier/Settings-<version>.xresources
```
