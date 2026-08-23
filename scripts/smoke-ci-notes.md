# Smoke tests & CI notes

`scripts/smoke.sh` is the GUI smoke test for the desktop app: it builds
`chm-app`, launches it with `CHM_SMOKE=1` on an X display (default `:1`,
falling back to `xvfb-run -a -s "-screen 0 1440x900x24"` when `$DISPLAY` is
unset), waits for the window to map, screenshots each page and fails with a
nonzero exit code on blank shots, a crash, or a panic in the app log.

## Where it runs

- **CI (GitHub-hosted)** — optional; headless via xvfb. Needs
  `xvfb imagemagick wmctrl` in addition to the usual Linux build deps.
- **Self-hosted GPU desktop runner** — preferred. The runner already has a
  real X server on `:1` plus GPU-accelerated rendering, so the app exercises
  its real paint path instead of llvmpipe.

## Job snippet (self-hosted GPU runner, xvfb fallback)

```yaml
  gui-smoke:
    name: GUI smoke (desktop)
    needs: lint-test
    runs-on: [self-hosted, linux, desktop]   # X display :1 available; GPU present
    timeout-minutes: 20
    steps:
      - uses: actions/checkout@v4

      # Only needed if this runner has no persistent toolchain setup:
      - uses: dtolnay/rust-toolchain@stable

      # The script itself handles the xvfb-run fallback when DISPLAY is
      # unset; on our desktop runners DISPLAY=:1 is exported by systemd,
      # so we just pass it through.
      - name: Run GUI smoke test
        env:
          DISPLAY: :1            # drop to use the script's xvfb-run fallback
          SHOTS_DIR: shots
        run: |
          xset -display "$DISPLAY" q >/dev/null 2>&1 || {
            echo "::warning::DISPLAY $DISPLAY unreachable — smoke.sh will fall back to xvfb-run"
          }
          bash scripts/smoke.sh "$DISPLAY"

      - name: Upload screenshots on failure
        if: failure()
        uses: actions/upload-artifact@v4
        with:
          name: smoke-shots-${{ github.run_id }}
          path: shots/
          retention-days: 3
          if-no-files-found: ignore
```

Headless GitHub-hosted variant: same steps on `ubuntu-latest`, but unset
`DISPLAY` so the script picks `xvfb-run`; add
`sudo apt-get install -y --no-install-recommends xvfb imagemagick wmctrl`
to the deps step.

## Contract relied on by CI

- exits nonzero on any failure (blank screenshot, early exit, panic);
- writes PNGs to `$SHOTS_DIR` (default `shots/`);
- honors `DISPLAY` / first positional arg as target display;
- performs its own `cargo build -p chm-app`, so no separate build step is
  needed before calling it.
