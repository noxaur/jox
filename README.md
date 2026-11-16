# jox

A quiet Jellyfin client for macOS and Linux.

jox is built for watching. While something plays, the controls sit small at the edges with no backgrounds, and they fade away. The rest of the app follows the [CatWM](https://github.com/noxaur/CatWM) design system: a dark rail, plain media cards with the caption under the image, and a thin progress line.

jox began as a fork of [tsukimi](https://github.com/tsukinaha/tsukimi). It talks to Jellyfin only.

## Servers

jox opens the server you last used and stays on it. Use the account menu at the bottom of the rail to switch to another server, add one or manage them.

## roarr

Give a server its [roarr](https://github.com/noxaur/roarr) address under **Manage Servers… → Edit**. jox signs in to roarr with the same username and password as Jellyfin. With roarr set, jox also gets:

- **Get** on a show or movie page, to fetch missing episodes, a single season or the movie.
- **Not in Your Library** in search, for titles you don't have yet.
- **Downloads**, to follow what roarr is fetching until it reaches Jellyfin.
- **Requests**, to approve, decline, or approve and get Seerr requests.

## Install on macOS

jox uses GTK, libadwaita and mpv from Homebrew:

```bash
brew install rust meson ninja just blueprint-compiler gtk4 libadwaita mpv gstreamer webp-pixbuf-loader
just app
```

`just app` makes a release build and installs it as `/Applications/Jox.app`. To install somewhere else, pass a path: `just app ~/Applications/Jox.app`. The app links the Homebrew libraries, so keep them installed.

The first launch can take a minute while GStreamer builds its plugin registry.

On macOS, mpv draws through its OpenGL render API instead of the Wayland embedder. MPRIS is not available, and animated images show only their first frame.

## Build for development

```bash
just run
```

This builds into `build/` and runs jox from there.

Debug builds can be driven without the mouse or keyboard. Set `JOX_DEV_CONTROL` to the path of a FIFO and write lines to it, as `src/ui/widgets/dev_control.rs` describes. This is how agents test jox on a [CatWM](https://github.com/noxaur/CatWM) agent display.

## Linux

The Meson build, Flatpak manifest and RPM specs from tsukimi are kept and renamed. jox has no packages yet.

## mpv config

jox reads your mpv config files. See the [mpv manual](https://mpv.io/manual/master/#files).

## License

jox is licensed under the [GPLv3](https://www.gnu.org/licenses/gpl-3.0.html), like tsukimi.

### Credits

- [tsukimi](https://github.com/tsukinaha/tsukimi) by Inaha and its contributors, which jox is built on.
- [gtk4-rs](https://github.com/gtk-rs/gtk4-rs), [libadwaita](https://gitlab.gnome.org/GNOME/libadwaita/) and [mpv](https://github.com/mpv-player/mpv).
- The [CatWM design system](https://github.com/noxaur/CatWM/blob/main/docs/design/design-system.md), with the Alpino and Pilcrow Rounded fonts from [Fontshare](https://www.fontshare.com) by Indian Type Foundry, used under the [ITF Free Font License](https://www.fontshare.com/terms).

The developers of jox have no affiliation with any content providers.
