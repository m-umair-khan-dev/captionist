# Settings window end-to-end tests

Drives the Captionist settings window like a user or screen reader would
(AT-SPI actions and the keyboard), then checks the widget, GSettings and the
caption service, and times each step. It also checks that the window stays
responsive during slow work.

Everything runs in a private headless GNOME Shell with its own D-Bus,
settings, data, state and runtime folder, so your desktop and settings are
never touched. Models are a hard-linked copy of `~/.local/share/captionist`:
they take no extra disk space, and a test deleting one leaves yours alone.

## Run

```sh
scripts/cargo-docker.sh build --release -p captionist -p captionistd
tests/e2e/session.sh target/release/captionistd extension   # start the private session
tests/e2e/in.sh target/release/captionist &                  # open the settings window in it
tests/e2e/in.sh python3 tests/e2e/suite.py                   # all tests
tests/e2e/in.sh python3 tests/e2e/suite.py t_models t_race   # or some
tests/e2e/session.sh --stop
```

Leave out the arguments to `session.sh` to test the installed daemon and
extension instead. Screenshots and `results.json` go to
`~/.cache/captionist-e2e/shots`.

The Models tests download one small model (about 28 MB). If the PC has an
accelerator runtime installed, they also switch to it and back.

## Files

- `session.sh` / `in.sh`: start the private session and run commands in it.
- `supervise.sh`: restarts the service when it exits, like systemd's
  `Restart=on-failure`. The service exits on purpose to switch accelerators.
- `desk.py`: keyboard, pointer and screenshots through Mutter's
  RemoteDesktop and ScreenCast D-Bus APIs.
- `frames.py`: measures when the screen actually changes. AT-SPI state can
  lag behind what's on screen.
- `a11y.py`: AT-SPI helpers.
- `suite.py`: the tests.
