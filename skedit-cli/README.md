# skedit-cli

A command-line tool for programmatically editing Starlark build files (Buck2, Bazel). Add/remove entries in list attributes, create/delete rules, and list targets — all with automatic formatting.

## Install

```sh
cargo install --path .
```

## Global Options

| Flag | Default | Description |
|------|---------|-------------|
| `--lookfor <RULE>` | `rust_binary` | Rule type to target |
| `--attr <ATTR>` | `srcs` | List attribute to operate on |
| `--name <NAME>` | | Target a specific rule by its `name` attribute (required when multiple rules of the same type exist) |

## Commands

### `add` — Add entries to a list attribute

```sh
skedit-cli add BUILD main.rs lib.rs
skedit-cli --attr deps add BUILD ':utils' ':logging'
skedit-cli --lookfor cc_library --name mylib add BUILD helper.cc
```

Entries are sorted alphabetically on insert. If the attribute doesn't exist on the rule, it is created automatically. Lists with more than 2 entries are formatted as multi-line.

### `remove` — Remove entries from a list attribute

```sh
skedit-cli remove BUILD old.rs
skedit-cli --attr deps remove BUILD ':unused_dep'
```

If the list becomes empty after removal, the attribute is removed from the rule entirely.

### `list` — List entries in a list attribute

```sh
skedit-cli list BUILD
skedit-cli --attr deps list BUILD
skedit-cli --lookfor rust_library --name utils list BUILD
```

Prints one entry per line to stdout.

### `create` — Create a new rule

```sh
skedit-cli create BUILD --name my_app
skedit-cli --lookfor cc_library create BUILD --name mylib
skedit-cli --lookfor rust_test create BUILD --name my_test
```

Appends a new rule block to the file. Creates the file if it doesn't exist. Errors if a rule of the same type and name already exists.

### `delete` — Delete a rule

```sh
skedit-cli delete BUILD --name my_app
skedit-cli --lookfor cc_library delete BUILD --name mylib
```

Removes the entire rule block from the file.

### `rules` — List all rules in a file

```sh
skedit-cli rules BUILD
skedit-cli rules BUILD --buck
```

Flat output (default):
```
my_app
mylib
```

Buck2/Bazel format (`--buck`):
```
//path/to/pkg:my_app
//path/to/pkg:mylib
```

## Formatting

When a rule is modified, the entire rule block is reformatted with consistent indentation:

- 4-space indent for attributes
- 8-space indent for multi-line list entries
- `name = value` spacing is normalized
- Lists with 1-2 entries stay single-line: `["a.rs", "b.rs"]`
- Lists with 3+ entries go multi-line:
  ```
  srcs = [
      "a.rs",
      "b.rs",
      "c.rs",
  ],
  ```
- Entries are always sorted alphabetically

Only the modified rule is reformatted. Other rules in the file are left untouched.

## Examples

Build up a target from scratch:

```sh
# Create the rule
skedit-cli --lookfor rust_binary create BUILD --name my_app

# Add source files
skedit-cli --name my_app add BUILD src/main.rs src/lib.rs src/utils.rs

# Add dependencies
skedit-cli --name my_app --attr deps add BUILD ':common' ':logging'

# Check what we have
skedit-cli --name my_app list BUILD
skedit-cli --name my_app --attr deps list BUILD
```

Result:
```python
rust_binary(
    name = "my_app",
    srcs = [
        "src/lib.rs",
        "src/main.rs",
        "src/utils.rs",
    ],
    deps = [":common", ":logging"],
)
```
