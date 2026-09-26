# yt-dlp YouTube JavaScript Runtime Requirements

## Verdict

The advisor identified a real Flatpak packaging gap, but “Deno is strictly required for every stream” would overstate the evidence. With the pinned `yt-dlp` 2026.08.19 and Deno hidden from `PATH`, an anonymous test video still resolved to an Opus audio format (`251|https|webm|opus|none`). yt-dlp logged `JS runtimes: none` and warned that no runtime was available and some formats may be missing. With host Deno 2.9.7 visible, the same URL logged that it solved YouTube JS challenges with Deno and EJS scripts v0.8.0.

Thus the no-runtime fallback can extract some streams; it is deprecated and has degraded/incomplete format coverage. The probe does not establish uniform throttling or a 15–30 KB/s limit. It also does not prove the Flatpak succeeds: the probe ran on the host, not in the sandbox.

## Pinned dependency evidence

The project's `io.github.castrojo.Banshee.yaml` installs only the bare `yt_dlp` wheel, pinned at `2026.8.19`. The matching upstream `pyproject.toml` declares no mandatory dependencies. Its optional `default` extra includes `yt-dlp-ejs==0.8.0`; a separate `deno` extra declares `deno>=2.6.6`, and the pinned dependency group pins Deno to `2.9.5`. The wheel installs Python yt-dlp code, not an external Deno executable. The manifest therefore currently bundles neither the companion EJS package nor a JS runtime.

## Primary sources

- [yt-dlp 2026.08.19 `pyproject.toml`](https://github.com/yt-dlp/yt-dlp/blob/2026.08.19/pyproject.toml#L47-L70): optional `default` and `deno` dependencies, including `yt-dlp-ejs==0.8.0` and `deno>=2.6.6`.
- [yt-dlp 2026.08.19 `pyproject.toml`](https://github.com/yt-dlp/yt-dlp/blob/2026.08.19/pyproject.toml#L99-L101): pinned Deno version `2.9.5`.
- [Official EJS setup guide](https://github.com/yt-dlp/yt-dlp/wiki/EJS): YouTube extraction uses an external JavaScript runtime to solve challenges; Deno is recommended/default-enabled, with Node and QuickJS supported when explicitly enabled. The guide says the PyPI `default` extra supplies `yt-dlp-ejs`; a runtime is a separate dependency.
- [yt-dlp 2026.08.19 YouTube extractor](https://github.com/yt-dlp/yt-dlp/blob/2026.08.19/yt_dlp/extractor/youtube/_video.py#L2971-L2998): no-runtime client fallback and warning path. [Challenge handling](https://github.com/yt-dlp/yt-dlp/blob/2026.08.19/yt_dlp/extractor/youtube/_video.py#L3300-L3356) and [format filtering](https://github.com/yt-dlp/yt-dlp/blob/2026.08.19/yt_dlp/extractor/youtube/_video.py#L3569-L3590) document missing formats when challenges remain unsolved.

## Recommendation

Before claiming reliable Flatpak YouTube playback, bundle the matching `yt-dlp-ejs` package and a supported JS runtime (Deno is the upstream-recommended/default-enabled option), pinned and integrity-checked. Alternatively, explicitly accept degraded extraction without a runtime and test representative audio URLs inside the built Flatpak. Do not treat the host test as sandbox evidence.
