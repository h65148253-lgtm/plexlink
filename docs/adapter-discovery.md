# PLEXLINK docs

This repository currently contains the first working implementation stage for Windows adapter discovery.

## Current status

- Windows agent project structure created.
- Adapter enumeration module implemented.
- Interface kind and status classification added.
- IP and health metadata collection included.
- Linux fallback behavior allows the project to build in non-Windows CI environments.

## Next stage

The next step is a dedicated monitoring module for connection-health sampling and interface change detection.
