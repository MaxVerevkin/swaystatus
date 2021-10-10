# swaystatus

Status command for i3bar/swaybar written in async rust. Based on i3status-rust. 

# Compatibility

Refer to https://github.com/MaxVerevkin/swaystatus/issues/4 for `i3status-rust` compatibility.

# Differences

### Blocks

#### Music

While it lacks many configuration options, music block allows switching between different players with mouse wheel.

### Enhanced clicks handling abilities

Each block supports multiple `click` options to handle blocks' clicks.

Example:

```toml
[[block]]
block = "time"
[[block.click]]
button = "left" # Which button to handle
cmd = "kitty" # The shell command to run
sync = false # Whether to wait for command to finish before proceeding (default is false)
update = true # Whether to update the block after click (default is true)
```

There is also an experimental support for double clicks:

```toml
[[block]]
block = "time"
[[block.click]]
button = "double_left"
cmd = "alacritty"
```

### Hsv color support

It is possible to specify theme's colors in HSV color space instead of RGB. The format is `"hsv:<hue>:<saturation>:<value>[:<alpha>]"`, where hue is in range `0..360`, saturation value and alpha are in range `0..=100`.

```toml
[theme]
name = "modern"
[theme.overrides]
idle_bg = "hsv:190:60:30"
```
