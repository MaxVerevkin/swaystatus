# swaystatus

Status command for i3bar/swaybar written in async rust. Based on i3status-rust. 

# Blocks

Refer to https://github.com/MaxVerevkin/swaystatus/issues/4 for `i3status-rust` compatibility.

Each block supports multiple `click` options to handle bolcks' clicks.

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
