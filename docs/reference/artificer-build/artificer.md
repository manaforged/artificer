# Module `artificer`

| Item | Kind | Description |
| --- | --- | --- |
| [`CheckOpts`](#checkopts) | struct |  |
| [`InstallReport`](#installreport) | struct |  |
| [`Mods`](#mods) | struct |  |
| [`Report`](#report) | struct |  |
| [`Reported`](#reported) | struct |  |
| [`ServeRequest`](#serverequest) | struct |  |
| [`StoreStat`](#storestat) | struct |  |
| [`SweepReport`](#sweepreport) | struct |  |
| [`Targets`](#targets) | struct |  |
| [`TestOpts`](#testopts) | struct |  |
| [`TransferReport`](#transferreport) | struct |  |
| [`Unmodeled`](#unmodeled) | struct |  |
| [`ColorChoice`](#colorchoice) | enum |  |
| [`Location`](#location) | enum |  |
| [`RemoteSource`](#remotesource) | enum |  |
| [`RustcOutcome`](#rustcoutcome) | enum |  |
| [`ScriptOutcome`](#scriptoutcome) | enum |  |
| [`add_to_profiles`](#add_to_profiles) | fn |  |
| [`cargo_home`](#cargo_home) | fn |  |
| [`cargo_package`](#cargo_package) | fn |  |
| [`check`](#check) | fn |  |
| [`check_cmd`](#check_cmd) | fn |  |
| [`check_package`](#check_package) | fn |  |
| [`check_real_cargo`](#check_real_cargo) | fn |  |
| [`control_home`](#control_home) | fn |  |
| [`default_home`](#default_home) | fn |  |
| [`doctor`](#doctor) | fn |  |
| [`enabled`](#enabled) | fn |  |
| [`env_script`](#env_script) | fn |  |
| [`export`](#export) | fn |  |
| [`fallback_report`](#fallback_report) | fn |  |
| [`import`](#import) | fn |  |
| [`install`](#install) | fn |  |
| [`installed_profiles`](#installed_profiles) | fn |  |
| [`isolate`](#isolate) | fn |  |
| [`load_mods`](#load_mods) | fn |  |
| [`note_fallback`](#note_fallback) | fn |  |
| [`passthrough_reason`](#passthrough_reason) | fn |  |
| [`path_prepend`](#path_prepend) | fn |  |
| [`path_remove`](#path_remove) | fn |  |
| [`profile_line`](#profile_line) | fn |  |
| [`profiles`](#profiles) | fn |  |
| [`pull`](#pull) | fn |  |
| [`purge`](#purge) | fn |  |
| [`ready`](#ready) | fn |  |
| [`refresh_shim`](#refresh_shim) | fn |  |
| [`remote`](#remote) | fn |  |
| [`remove_from_profiles`](#remove_from_profiles) | fn |  |
| [`report_error`](#report_error) | fn |  |
| [`run_cmd`](#run_cmd) | fn |  |
| [`save_mods`](#save_mods) | fn |  |
| [`serve_listen`](#serve_listen) | fn |  |
| [`serve_ping`](#serve_ping) | fn |  |
| [`serve_stop`](#serve_stop) | fn |  |
| [`serve_try`](#serve_try) | fn |  |
| [`set_color`](#set_color) | fn |  |
| [`set_jobs`](#set_jobs) | fn |  |
| [`set_quiet`](#set_quiet) | fn |  |
| [`set_remote`](#set_remote) | fn |  |
| [`set_trace`](#set_trace) | fn |  |
| [`spawn_pull`](#spawn_pull) | fn |  |
| [`stock_cargo`](#stock_cargo) | fn |  |
| [`store_stat`](#store_stat) | fn |  |
| [`sweep_dir`](#sweep_dir) | fn |  |
| [`test_package`](#test_package) | fn |  |
| [`toolchain_path`](#toolchain_path) | fn |  |
| [`uninstall`](#uninstall) | fn |  |
| [`why_miss`](#why_miss) | fn |  |
| [`LAYOUT`](#layout) | const |  |
| [`PULL_EVERY`](#pull_every) | const |  |
| [`REMOTE_ENV`](#remote_env) | const |  |

## Structs

### `CheckOpts`

**Fields**

| Field | Description |
| --- | --- |
| <code>all_features: bool</code> |  |
| <code>features: Vec&lt;String&gt;</code> |  |
| <code>json: bool</code> |  |
| <code>link: bool</code> |  |
| <code>meta_flags: Vec&lt;String&gt;</code> |  |
| <code>no_default: bool</code> |  |
| <code>release: bool</code> |  |
| <code>target_dir: Option&lt;PathBuf&gt;</code> |  |
| <code>targets: <a href="#targets">Targets</a></code> |  |
| <code>workspace: bool</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Default</code>, <code>Debug</code>


### `InstallReport`

**Fields**

| Field | Description |
| --- | --- |
| <code>binary: PathBuf</code> |  |
| <code>env: PathBuf</code> |  |
| <code>shim: PathBuf</code> |  |


### `Mods`

**Methods**

| Method | Description |
| --- | --- |
| <code>get(&amp;self, name: &amp;str) -&gt; Result&lt;bool&gt;</code> |  |
| <code>names() -&gt; &amp;'static [&amp;'static str]</code> |  |
| <code>set(&amp;mut self, name: &amp;str, on: bool) -&gt; Result&lt;()&gt;</code> |  |
| <code>table(&amp;self) -&gt; BTreeMap&lt;&amp;'static str, bool&gt;</code> |  |

**Fields**

| Field | Description |
| --- | --- |
| <code>cranelift: bool</code> |  |
| <code>enabled: bool</code> |  |
| <code>linker: bool</code> |  |
| <code>meta_cache: bool</code> |  |
| <code>rmeta: bool</code> |  |
| <code>serve: bool</code> |  |
| <code>slim: bool</code> |  |
| <code>sweep: bool</code> |  |
| <code>threads: bool</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Default</code>, <code>Debug</code>, <code>Serialize</code>, <code>Deserialize&lt;'de&gt;</code>


### `Report`

**Fields**

| Field | Description |
| --- | --- |
| <code>rlib: PathBuf</code> |  |
| <code>rustc: <a href="#rustcoutcome">RustcOutcome</a></code> |  |
| <code>script: <a href="#scriptoutcome">ScriptOutcome</a></code> |  |

**Trait implementations:** <code>Debug</code>


### `Reported`

**Trait implementations:** <code>Error</code>, <code>Debug</code>, <code>Display</code>


### `ServeRequest`

**Fields**

| Field | Description |
| --- | --- |
| <code>all_features: bool</code> |  |
| <code>all_targets: bool</code> |  |
| <code>args: Vec&lt;String&gt;</code> |  |
| <code>dir: PathBuf</code> |  |
| <code>doc: bool</code> |  |
| <code>features: Vec&lt;String&gt;</code> |  |
| <code>json: bool</code> |  |
| <code>lib: bool</code> |  |
| <code>link: bool</code> |  |
| <code>meta_flags: Vec&lt;String&gt;</code> |  |
| <code>no_default: bool</code> |  |
| <code>no_run: bool</code> |  |
| <code>only: Vec&lt;String&gt;</code> |  |
| <code>op: String</code> |  |
| <code>packages: Vec&lt;String&gt;</code> |  |
| <code>release: bool</code> |  |
| <code>target_dir: Option&lt;PathBuf&gt;</code> |  |
| <code>tests: bool</code> |  |
| <code>token: String</code> |  |
| <code>workspace: bool</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Debug</code>, <code>Serialize</code>, <code>Deserialize&lt;'de&gt;</code>


### `StoreStat`

**Fields**

| Field | Description |
| --- | --- |
| <code>builds: u64</code> |  |
| <code>bytes: u64</code> |  |
| <code>fallback_last: Option&lt;String&gt;</code> |  |
| <code>fallbacks: u64</code> |  |
| <code>hits: u64</code> |  |
| <code>last_build: Option&lt;String&gt;</code> |  |
| <code>meta: u32</code> |  |
| <code>meta_bytes: u64</code> |  |
| <code>misses: u64</code> |  |
| <code>scratch: u32</code> |  |
| <code>scratch_bytes: u64</code> |  |
| <code>units: u32</code> |  |

**Trait implementations:** <code>Default</code>, <code>Debug</code>


### `SweepReport`

**Fields**

| Field | Description |
| --- | --- |
| <code>evicted_bytes: u64</code> |  |
| <code>evicted_units: u32</code> |  |
| <code>incremental_dirs: u32</code> |  |
| <code>scratch_dirs: u32</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Eq</code>, <code>PartialEq</code>, <code>Default</code>, <code>Debug</code>, <code>Copy</code>, <code>StructuralPartialEq</code>


### `Targets`

**Fields**

| Field | Description |
| --- | --- |
| <code>all: bool</code> |  |
| <code>tests: bool</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Default</code>, <code>Debug</code>, <code>Copy</code>


### `TestOpts`

**Fields**

| Field | Description |
| --- | --- |
| <code>all_features: bool</code> |  |
| <code>args: Vec&lt;String&gt;</code> |  |
| <code>doc: bool</code> |  |
| <code>features: Vec&lt;String&gt;</code> |  |
| <code>json: bool</code> |  |
| <code>lib: bool</code> |  |
| <code>meta_flags: Vec&lt;String&gt;</code> |  |
| <code>no_default: bool</code> |  |
| <code>no_run: bool</code> |  |
| <code>only: Vec&lt;String&gt;</code> |  |
| <code>release: bool</code> |  |
| <code>target_dir: Option&lt;PathBuf&gt;</code> |  |
| <code>workspace: bool</code> |  |

**Trait implementations:** <code>Default</code>, <code>Debug</code>


### `TransferReport`

**Fields**

| Field | Description |
| --- | --- |
| <code>bytes: u64</code> |  |
| <code>units: u64</code> |  |


### `Unmodeled`

**Trait implementations:** <code>Error</code>, <code>Debug</code>, <code>Display</code>


## Enums

### `ColorChoice`

**Methods**

| Method | Description |
| --- | --- |
| <code>parse(value: &amp;str) -&gt; Option&lt;Self&gt;</code> |  |

**Variants**

| Variant | Description |
| --- | --- |
| <code>Always</code> |  |
| <code>Auto</code> |  |
| <code>Never</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Eq</code>, <code>PartialEq</code>, <code>Debug</code>, <code>Copy</code>, <code>StructuralPartialEq</code>


### `Location`

**Methods**

| Method | Description |
| --- | --- |
| <code>parse(raw: &amp;str) -&gt; Result&lt;Self&gt;</code> |  |

**Fields**

| Field | Description |
| --- | --- |
| <code>Ssh::host: String</code> |  |
| <code>Ssh::path: String</code> |  |

**Variants**

| Variant | Description |
| --- | --- |
| <code>Dir(PathBuf)</code> |  |
| <code>Ssh</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Eq</code>, <code>PartialEq</code>, <code>Debug</code>, <code>Display</code>, <code>StructuralPartialEq</code>


### `RemoteSource`

**Variants**

| Variant | Description |
| --- | --- |
| <code>Config</code> |  |
| <code>Env</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Eq</code>, <code>PartialEq</code>, <code>Debug</code>, <code>Copy</code>, <code>StructuralPartialEq</code>


### `RustcOutcome`

**Variants**

| Variant | Description |
| --- | --- |
| <code>Ran</code> |  |
| <code>Restored</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Eq</code>, <code>PartialEq</code>, <code>Debug</code>, <code>Copy</code>, <code>StructuralPartialEq</code>


### `ScriptOutcome`

**Variants**

| Variant | Description |
| --- | --- |
| <code>None</code> |  |
| <code>Ran</code> |  |
| <code>Restored</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Eq</code>, <code>PartialEq</code>, <code>Debug</code>, <code>Copy</code>, <code>StructuralPartialEq</code>


## Functions

### `add_to_profiles`

<pre>pub fn <a href="#add_to_profiles">add_to_profiles</a>(files: &amp;[PathBuf], line: &amp;str, control: &amp;Path) -&gt; Result&lt;Vec&lt;PathBuf&gt;&gt;</pre>


### `cargo_home`

<pre>pub fn <a href="#cargo_home">cargo_home</a>() -&gt; PathBuf</pre>


### `cargo_package`

<pre>pub fn <a href="#cargo_package">cargo_package</a>(cargo_home: &amp;Path) -&gt; Option&lt;String&gt;</pre>


### `check`

<pre>pub fn <a href="#check">check</a>(pkg: &amp;Path, home: &amp;Path) -&gt; Result&lt;<a href="#report">Report</a>&gt;</pre>


### `check_cmd`

<pre>pub fn <a href="#check_cmd">check_cmd</a>(dir: &amp;Path, packages: &amp;[String], home: &amp;Path, opts: <a href="#checkopts">CheckOpts</a>) -&gt; Result&lt;i32&gt;</pre>


### `check_package`

<pre>pub fn <a href="#check_package">check_package</a>(dir: &amp;Path, packages: &amp;[String], home: &amp;Path) -&gt; Result&lt;<a href="#report">Report</a>&gt;</pre>


### `check_real_cargo`

<pre>pub fn <a href="#check_real_cargo">check_real_cargo</a>(real_cargo: &amp;Path, control: &amp;Path) -&gt; Result&lt;()&gt;</pre>


### `control_home`

<pre>pub fn <a href="#control_home">control_home</a>() -&gt; PathBuf</pre>


### `default_home`

<pre>pub fn <a href="#default_home">default_home</a>() -&gt; PathBuf</pre>


### `doctor`

<pre>pub fn <a href="#doctor">doctor</a>(home: &amp;Path) -&gt; Result&lt;i32&gt;</pre>


### `enabled`

<pre>pub fn <a href="#enabled">enabled</a>(home: &amp;Path) -&gt; Result&lt;bool&gt;</pre>


### `env_script`

<pre>pub fn <a href="#env_script">env_script</a>() -&gt; String</pre>


### `export`

<pre>pub fn <a href="#export">export</a>(home: &amp;Path, dest: &amp;Path, days: u64, max_bytes: u64) -&gt; Result&lt;<a href="#transferreport">TransferReport</a>&gt;</pre>


### `fallback_report`

<pre>pub fn <a href="#fallback_report">fallback_report</a>(home: &amp;Path, limit: usize) -&gt; String</pre>


### `import`

<pre>pub fn <a href="#import">import</a>(home: &amp;Path, src: &amp;Path) -&gt; Result&lt;<a href="#transferreport">TransferReport</a>&gt;</pre>


### `install`

<pre>pub fn <a href="#install">install</a>(binary: &amp;Path, real_cargo: &amp;Path, cargo_home: &amp;Path, control: &amp;Path) -&gt; Result&lt;<a href="#installreport">InstallReport</a>&gt;</pre>


### `installed_profiles`

<pre>pub fn <a href="#installed_profiles">installed_profiles</a>(home: &amp;Path, zdotdir: Option&lt;&amp;Path&gt;) -&gt; Vec&lt;PathBuf&gt;</pre>


### `isolate`

<pre>pub fn <a href="#isolate">isolate</a>(cmd: &amp;mut Command)</pre>


### `load_mods`

<pre>pub fn <a href="#load_mods">load_mods</a>(home: &amp;Path) -&gt; Result&lt;<a href="#mods">Mods</a>&gt;</pre>


### `note_fallback`

<pre>pub fn <a href="#note_fallback">note_fallback</a>(home: &amp;Path, reason: &amp;str)</pre>


### `passthrough_reason`

<pre>pub fn <a href="#passthrough_reason">passthrough_reason</a>(req: &amp;<a href="#serverequest">ServeRequest</a>, dev: bool, home: &amp;Path) -&gt; Result&lt;Option&lt;String&gt;&gt;</pre>


### `path_prepend`

<pre>pub fn <a href="#path_prepend">path_prepend</a>(path: &amp;str, dir: &amp;str) -&gt; Option&lt;String&gt;</pre>


### `path_remove`

<pre>pub fn <a href="#path_remove">path_remove</a>(path: &amp;str, dir: &amp;str) -&gt; Option&lt;String&gt;</pre>


### `profile_line`

<pre>pub fn <a href="#profile_line">profile_line</a>(home: &amp;Path, control: &amp;Path) -&gt; String</pre>


### `profiles`

<pre>pub fn <a href="#profiles">profiles</a>(home: &amp;Path, shell: Option&lt;&amp;Path&gt;, zdotdir: Option&lt;&amp;Path&gt;) -&gt; Vec&lt;PathBuf&gt;</pre>


### `pull`

<pre>pub fn <a href="#pull">pull</a>(home: &amp;Path) -&gt; Result&lt;(<a href="#location">Location</a>, <a href="#transferreport">TransferReport</a>)&gt;</pre>


### `purge`

<pre>pub fn <a href="#purge">purge</a>(home: &amp;Path) -&gt; Result&lt;bool&gt;</pre>


### `ready`

<pre>pub fn <a href="#ready">ready</a>(home: &amp;Path) -&gt; bool</pre>


### `refresh_shim`

<pre>pub fn <a href="#refresh_shim">refresh_shim</a>(running: &amp;Path, control: &amp;Path, cargo_home: &amp;Path) -&gt; Result&lt;bool&gt;</pre>


### `remote`

<pre>pub fn <a href="#remote">remote</a>(home: &amp;Path) -&gt; Result&lt;Option&lt;(<a href="#location">Location</a>, <a href="#remotesource">RemoteSource</a>)&gt;&gt;</pre>


### `remove_from_profiles`

<pre>pub fn <a href="#remove_from_profiles">remove_from_profiles</a>(files: &amp;[PathBuf], line: &amp;str, control: &amp;Path) -&gt; Result&lt;Vec&lt;PathBuf&gt;&gt;</pre>


### `report_error`

<pre>pub fn <a href="#report_error">report_error</a>(message: impl Display)</pre>


### `run_cmd`

<pre>pub fn <a href="#run_cmd">run_cmd</a>(dir: &amp;Path, packages: &amp;[String], bin: Option&lt;&amp;str&gt;, example: Option&lt;&amp;str&gt;, home: &amp;Path, opts: <a href="#checkopts">CheckOpts</a>, args: &amp;[String]) -&gt; Result&lt;i32&gt;</pre>


### `save_mods`

<pre>pub fn <a href="#save_mods">save_mods</a>(home: &amp;Path, mods: &amp;<a href="#mods">Mods</a>) -&gt; Result&lt;()&gt;</pre>


### `serve_listen`

<pre>pub fn <a href="#serve_listen">serve_listen</a>(home: &amp;Path) -&gt; Result&lt;()&gt;</pre>


### `serve_ping`

<pre>pub fn <a href="#serve_ping">serve_ping</a>(home: &amp;Path) -&gt; bool</pre>


### `serve_stop`

<pre>pub fn <a href="#serve_stop">serve_stop</a>(home: &amp;Path) -&gt; Result&lt;()&gt;</pre>


### `serve_try`

<pre>pub fn <a href="#serve_try">serve_try</a>(home: &amp;Path, req: &amp;mut <a href="#serverequest">ServeRequest</a>) -&gt; Option&lt;Result&lt;i32&gt;&gt;</pre>


### `set_color`

<pre>pub fn <a href="#set_color">set_color</a>(choice: <a href="#colorchoice">ColorChoice</a>)</pre>


### `set_jobs`

<pre>pub fn <a href="#set_jobs">set_jobs</a>(jobs: usize)</pre>


### `set_quiet`

<pre>pub fn <a href="#set_quiet">set_quiet</a>(on: bool)</pre>


### `set_remote`

<pre>pub fn <a href="#set_remote">set_remote</a>(home: &amp;Path, location: Option&lt;&amp;<a href="#location">Location</a>&gt;) -&gt; Result&lt;()&gt;</pre>


### `set_trace`

<pre>pub fn <a href="#set_trace">set_trace</a>(on: bool)</pre>


### `spawn_pull`

<pre>pub fn <a href="#spawn_pull">spawn_pull</a>(home: &amp;Path) -&gt; Result&lt;()&gt;</pre>


### `stock_cargo`

<pre>pub fn <a href="#stock_cargo">stock_cargo</a>() -&gt; PathBuf</pre>


### `store_stat`

<pre>pub fn <a href="#store_stat">store_stat</a>(home: &amp;Path) -&gt; Result&lt;<a href="#storestat">StoreStat</a>&gt;</pre>


### `sweep_dir`

<pre>pub fn <a href="#sweep_dir">sweep_dir</a>(dir: &amp;Path, target: Option&lt;&amp;Path&gt;, home: &amp;Path) -&gt; Result&lt;<a href="#sweepreport">SweepReport</a>&gt;</pre>


### `test_package`

<pre>pub fn <a href="#test_package">test_package</a>(dir: &amp;Path, packages: &amp;[String], home: &amp;Path, opts: &amp;<a href="#testopts">TestOpts</a>) -&gt; Result&lt;i32&gt;</pre>


### `toolchain_path`

<pre>pub fn <a href="#toolchain_path">toolchain_path</a>() -&gt; Option&lt;OsString&gt;</pre>


### `uninstall`

<pre>pub fn <a href="#uninstall">uninstall</a>(cargo_home: &amp;Path, control: &amp;Path) -&gt; Result&lt;Vec&lt;PathBuf&gt;&gt;</pre>


### `why_miss`

<pre>pub fn <a href="#why_miss">why_miss</a>(home: &amp;Path, crate_name: &amp;str) -&gt; Option&lt;String&gt;</pre>


## Constants

### `LAYOUT`

<pre>pub const <a href="#layout">LAYOUT</a>: &amp;str</pre>


### `PULL_EVERY`

<pre>pub const <a href="#pull_every">PULL_EVERY</a>: Duration</pre>


### `REMOTE_ENV`

<pre>pub const <a href="#remote_env">REMOTE_ENV</a>: &amp;str</pre>


---

Generated by truesight from `artificer-build` 0.1.1. See the [API overview](index.md).
