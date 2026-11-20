# 規劃：Fagus 的 C ABI 動態庫（PHP FFI，7.4–8.4）

狀態：草案，尚未實作
日期：2026-11-20
範圍：`crates/fagus-ffi`、`crates/fagus-protocol`、`php/fagus-ffi`、`.github/workflows/release.yml`
非範圍：worker 協定的移除、PHP extension（`ext-fagus`）、GoReleaser 導入

## 1. 摘要

把 Fagus 的渲染引擎以 C ABI 動態庫（Rust `cdylib`）的形式提供給 PHP 的 FFI 擴充使用，
取代（而不是取代掉）目前的 worker 子程序模式。目標平台與 PHP 版本：

| 平台 | 產物 | PHP |
|---|---|---|
| Linux x86_64 (glibc) | `libfagus_ffi.so` | 7.4 / 8.0 / 8.3 / 8.4 |
| Linux aarch64 (glibc) | `libfagus_ffi.so` | 7.4 / 8.0 / 8.3 / 8.4 |
| Windows x86_64 (MSVC) | `fagus_ffi.dll` | 7.4 / 8.0 / 8.3 / 8.4 |

核心結論：**三個 native 產物就夠，不需要為每個 PHP 版本各建一份。**
FFI 載入的是普通 C ABI 動態庫，不經過 Zend API，因此不需要與 PHP 的版本、編譯器、
ZTS/NTS 對齊；PHP 版本差異只存在於 PHP 端的包裝程式碼。

「只需要改 GitHub Actions」是**不成立**的：目前 workspace 沒有任何 `cdylib` target、
沒有 C ABI、PHP 端也沒有 FFI 實作，CI 沒有東西可建。

## 2. 現況與證據

| 事實 | 位置 |
|---|---|
| 引擎已經是獨立函式庫（`Engine` / `Document` / `Output`） | `crates/fagus/src/lib.rs:577`, `docs/rust.md` |
| CLI 只是 clap + sandbox + worker 協定 | `crates/fagus-cli/src/main.rs`, `crates/fagus-cli/src/worker.rs` |
| worker 的請求結構是 binary crate 的私有型別 | `crates/fagus-cli/src/worker.rs:138` |
| CLI 的 render 命令另外手寫一份 `Document` 映射 | `crates/fagus-cli/src/main.rs:524-611` |
| 簽章、時間戳、吊銷都是呼叫端提供的閉包 | `crates/fagus-cli/src/worker.rs:674`, `:688`, `:713` |
| 沙箱是 process 級（Landlock / seccomp / rlimit） | `crates/fagus-cli/src/sandbox.rs`, `memory.rs` |
| release 用 `panic = "unwind"` | `Cargo.toml:154` |
| workspace `unsafe_code = "deny"` | `Cargo.toml:129` |
| 現有 PHP 套件要求 PHP 8.3，且使用 8.3 語法 | `composer.json:32`, `php/fagus/src/Internal/Release.php:16` |
| Linux 已用 zigbuild 固定 glibc 2.17 | `.github/workflows/release.yml:34-37` |

## 3. 架構決策

### 3.1 新增 `crates/fagus-ffi`

不修改 CLI；在 `fagus` crate 之上加一層薄 C ABI。

```toml
[package]
name = "fagus-ffi"
# …

[lib]
name = "fagus_ffi"
crate-type = ["cdylib", "staticlib", "rlib"]

[lints.rust]
# 覆寫 workspace 的 deny：這是唯一允許 unsafe 的 crate
unsafe_code = "allow"
```

`rlib` 是為了讓 `cargo test -p fagus-ffi` 能在 Rust 端直接呼叫 ABI 函式。
`staticlib` 是給需要靜態連結的呼叫端（非 PHP）。

### 3.2 新增 `crates/fagus-protocol`

把 worker 的 JSON 請求 → `Document` 映射抽出來，讓 CLI 與 FFI 共用，避免第三份會漂移的實作。

```
fagus-protocol（依賴 fagus + serde + serde_json）
├── Request / Stylesheet / Resource / Attachment / SignatureRequest / Area
├── impl Request { fn to_document(&self, blobs: &[&[u8]]) -> Result<Document, Failure> }
└── Response：pages / diagnostics / timings / layout → JSON
```

行為不變；`worker.rs` 內既有的 protocol 測試直接當回歸測試。

### 3.3 新增 `php/fagus-ffi`

現有 `php/fagus` 無法下修到 7.4（typed constant、enum、readonly、constructor promotion…），
因此 7.4 必須是獨立套件，語法鎖在 7.4：

```
php/fagus-ffi/
├── composer.json     # "php": ">=7.4", "ext-ffi": "*"
├── src/Native.php    # FFI::cdef 一次；引擎 handle；render
├── src/Engine.php
├── src/Pdf.php
├── src/Exception/*.php
└── tests/smoke.php   # 純 PHP 腳本（PHPUnit 12 要 8.3，不能用）
```

定位為低階層；未來 8.3 的 `php/fagus` 可以選擇性依賴它，把 FFI 當第二種 transport。

### 3.4 第一階段不做 callback

`S`（外部簽章）、`T`（時間戳）、`V`（OCSP/CRL）需要 function pointer。
PHP 官方手冊明確指出 callback「not supported on all libffi platforms, not efficient and
leaks resources by the end of request」，在 7.4–8.4 全支援的目標下不適合當第一階段。

好消息：**PKCS#12 簽章不需要 callback**（`SignatureRequest.pkcs12` 是 blob 索引），
所以帶簽章的 PDF 在第一階段就能支援；缺的只有外部簽章器、時間戳與吊銷查詢。

## 4. C ABI 規格

設計原則：

1. **沒有 struct**，只傳純量與位元組緩衝區 → 不存在 MSVC/GCC 的 layout 差異。
2. 記憶體一律 Rust 配置、Rust 釋放 → 與 `+crt-static` 相容。
3. 輸入一律 `(ptr, len)`，不用 C 字串（PHP 字串可含 NUL）。
4. 每個入口包 `catch_unwind`，錯誤經 thread-local 回報。

```c
#ifndef FAGUS_FFI_H
#define FAGUS_FFI_H

#include <stdint.h>
#include <stddef.h>

/* ABI 版本：載入後先檢查，避免舊 .so 被新 wrapper 使用。 */
uint32_t    fagus_abi_version(void);
const char *fagus_version(void);

/* config_json：引擎設定（字型目錄、locale、generic families、limits）。 */
void       *fagus_engine_new(const uint8_t *config_json, size_t config_len);
void        fagus_engine_free(void *engine);

/*
 * request_json：與 worker 協定相同的請求標頭，另加 "blobLengths": [..]。
 * blob_data：所有 blob 串接（HTML、樣式表、資源、附件、發票 XML、字型）。
 * 回傳 0 表示成功，其他為錯誤碼；錯誤訊息由 fagus_last_error() 取得。
 */
int32_t     fagus_render(void *engine,
                         const uint8_t *request_json, size_t request_len,
                         const uint8_t *blob_data,    size_t blob_data_len,
                         uint8_t **pdf,  size_t *pdf_len,
                         uint8_t **meta, size_t *meta_len);

/* 釋放 fagus_render 回傳的緩衝區。len 必須是回傳時的長度。 */
void        fagus_bytes_free(uint8_t *ptr, size_t len);

/* thread-local 的最後一個錯誤訊息，NUL 結尾，由 Fagus 持有。 */
const char *fagus_last_error(void);

#endif
```

`meta` 是 JSON：`pages`、`diagnostics`、`timings`、`layout`。用 JSON 取代十幾個 getter，
避免 ABI 表面積膨脹。

## 5. PHP 端用法（7.4 語法）

```php
$ffi = FFI::cdef(file_get_contents(__DIR__.'/fagus.h'), '/usr/lib/libfagus_ffi.so');

$engine = $ffi->fagus_engine_new($configJson, strlen($configJson));

$pdfPtr = $ffi->new('uint8_t*');
$pdfLen = $ffi->new('size_t');
$metaPtr = $ffi->new('uint8_t*');
$metaLen = $ffi->new('size_t');

$status = $ffi->fagus_render(
    $engine,
    $requestJson, strlen($requestJson),
    $blobData,    strlen($blobData),
    FFI::addr($pdfPtr), FFI::addr($pdfLen),
    FFI::addr($metaPtr), FFI::addr($metaLen)
);

if ($status !== 0) {
    throw new RuntimeException(FFI::string($ffi->fagus_last_error()));
}

$pdf = FFI::string($pdfPtr, $pdfLen->cdata);
$ffi->fagus_bytes_free($pdfPtr, $pdfLen->cdata);
```

PHP 7.4 的實作限制：

- `ffi.enable` 預設是 `preload`，CLI / FPM 需設 `ffi.enable=true`（或寫入 preload 檔）。
- Windows 需 `extension=ffi`（官方 build 內含 `php_ffi.dll`）。
- 表頭避免 C99 `bool`；用 `uint32_t` 回傳 0/1。
- PHP 7.4 把 `const char *` 的回傳值直接變成 PHP `string`，不是 `FFI\CData`；包裝要用
  `is_string($v) ? $v : FFI::string($v)`，8.x 之後才可能是 `CData`（spike 已驗證，見 §13）。
- PHP 7.4 的 C 宣告解析器較舊，`fagus.h` 要用保守語法（不用 `_Bool`、不依賴複雜 typedef），
  且 CI 必須真的在 7.4 上 `FFI::cdef` 成功一次，不能只靠編譯通過（spike 已通過）。
- 建議使用 NTS build（CLI 與 php-fpm 預設）。ZTS（Apache `mod_php`）不建議：
  PHP 手冊說 callback「not supported on all libffi platforms」，ZTS 會讓風險更高。
- PHP 7.4 自 2022-11 起 EOL，定位為「客戶環境支援」，不是推薦版本。

## 6. 平台細節

### 6.1 Windows（MSVC）

FFI 動態庫不是 PHP extension，不需要與 PHP 同一個 MSVC。真正要處理的是 CRT。官方
Windows build 使用的編譯器（php.net）：

| PHP | Visual Studio | 需要的 CRT redist |
|---|---|---|
| 7.2–7.4 | VS15（2017） | VC++ 2015-2019 |
| 8.0–8.3 | VS16（2019） | VC++ 2015-2019 |
| 8.4–8.5 | VS17（2022） | VC++ 2015-2022 |

一份 DLL 要同時服務這些版本，實務上可行：VC++ 2015-2022 redistributable 是同一條
binary-compatible 產品線。但它仍然要求使用者的機器裝了夠新的 redist，而 Windows Server
或精簡容器常常沒有。`+crt-static` 把這個變數完全移除，因此是本規劃的預設：

```toml
# .cargo/config.toml
[target.x86_64-pc-windows-msvc]
rustflags = ["-C", "target-feature=+crt-static"]
```

或只針對此 crate：

```bash
cargo rustc -p fagus-ffi --release --target x86_64-pc-windows-msvc \
  -- -C target-feature=+crt-static
```

注意：不要用全域 `RUSTFLAGS`，它會一併套用到 build script 與 proc-macro
（[rust-lang/rust#71651](https://github.com/rust-lang/rust/issues/71651)）；用
`[target.…] rustflags` 或 `cargo rustc` 只作用在目標 crate。

`+crt-static` 用在 Windows `cdylib` 是受支援的：Rust 的 MSVC 目標允許 DLL 自帶靜態 CRT，
前提是不要跨 DLL 邊界交換 CRT 物件（記憶體、`FILE*`、`errno`）。本 ABI 只交換位元組緩衝區，
且一律 Rust 配置、Rust 釋放。

驗收（CI）：

```powershell
dumpbin /dependents fagus_ffi.dll
# 不得出現 VCRUNTIME140.dll / MSVCP140.dll / ucrtbase.dll
```

32-bit PHP：官方 7.4 Windows build 有 x86。若不支援，文件與 CI 明確寫死 x64；
若要支援，需加 `i686-pc-windows-msvc` target。

### 6.2 Linux（glibc）

沿用現有 zigbuild 策略：

- `x86_64-unknown-linux-gnu.2.17`
- `aarch64-unknown-linux-gnu.2.17`

涵蓋 Debian 11（2.31）、12（2.36）、13（2.41）。**不建 musl 版本。**

驗收：

```bash
readelf --version-info libfagus_ffi.so   # 最高 GLIBC_2.17
nm -D --defined-only libfagus_ffi.so     # 只匯出 fagus_*
```

### 6.3 依賴審計

`Cargo.lock` 中唯一的 `-sys` crate 是 `windows-sys`；`fontdb` 的 fontconfig 功能使用純 Rust 的
`fontconfig-parser` 讀取 `/etc/fonts/fonts.conf`（不是 C 的 libfontconfig）。因此 Linux 動態庫
不需要 `libfontconfig` / freetype，執行期依賴只有 `libc`、`libm`、`libgcc_s`、`ld-linux`。
精簡映像（`debian:slim`）要確認有 `libgcc-s1`，否則 Rust 的 unwind 會載入失敗。

## 7. CI/CD 變更（只改 flow 的部分）

`.github/workflows/release.yml`：

1. 新增 `lib` job，matrix 只有三個 target（2 個 gnu + 1 個 msvc），沿用現有 zig/rustup 步驟。
2. Windows 加 `+crt-static`，並跑 `dumpbin /dependents` 檢查。
3. 產出 `fagus.h`（cbindgen 只做同步檢查，不當 build 依賴）。
4. 打包：`fagus-ffi-<target>.tar.gz` / `.zip`，內含 `.so`/`.dll` + `fagus.h`。
5. 併入既有 draft release 的 `SHA256SUMS`、attestation 與 `tools/pin-binaries.php` 流程。
6. musl 不列入 `lib` job。

PHP 相容性測試：

| 平台 | 方式 |
|---|---|
| Linux x64 | Docker `php:7.4-cli` / `8.0-cli` / `8.3-cli` / `8.4-cli` 跑 `tests/smoke.php` |
| Linux arm64 | `ubuntu-24.04-arm` runner，至少驗 PHP 8.4 |
| Windows x64 | `shivammathur/setup-php`，`php-version: 7.4` 與 `8.4`，`ini-values: ffi.enable=true` |

官方 `php:*-cli` 映像**沒有內建 FFI**，要在 Dockerfile 內建置：

```dockerfile
RUN apt-get update \
 && apt-get install -y --no-install-recommends libffi-dev libgcc-s1 \
 && docker-php-ext-install ffi \
 && rm -rf /var/lib/apt/lists/* \
 && printf 'ffi.enable=true\n' > /usr/local/etc/php/conf.d/ffi.ini
```

每個 job 第一步先 assert：

```bash
php -r 'exit(extension_loaded("ffi") ? 0 : 1);'
```

不要假設映像一定帶 FFI；`docker-php-ext-install ffi` 需要 `libffi-dev`，執行期需要 `libgcc-s1`。

確定性驗收：同一份 HTML + 同一個 `--created`，FFI 輸出必須與 CLI 輸出位元組相同。

## 8. 風險與取捨

1. **沙箱消失。** Landlock / seccomp / rlimit 是 process 級，in-process 會把 PHP-FPM 一起限制住。
   FFI 路徑不能處理不可信 HTML；worker 模式保留為預設安全路徑。需寫入 `SECURITY.md`。
2. **panic / OOM 會殺掉整個 FPM worker。** `catch_unwind` 擋得住 panic，擋不住 abort 與 OOM。
   FFI 模式應強制設定 `Limits`，不可預設 `unlimited`。
3. **記憶體乘以 FPM process 數。** 每個 child 各有一份 `Engine`（`FaceCache` 上限 256 MiB，
   `crates/fagus/src/lib.rs:612`）。以 `pm.max_requests` 或固定 worker 數控制。
4. **ABI 穩定性。** `fagus_abi_version()` 必須在載入時檢查；不支援跨大版本混用。
5. **7.4 維護成本。** 兩套 PHP 套件（7.4 精簡版、8.3 完整版）需各自測試。

## 9. 里程碑

| 階段 | 內容 | 估時 | 驗收 | 狀態 |
|---|---|---|---|---|
| M1 | `fagus-protocol` 抽取、`fagus-ffi`、`fagus.h` | 1–1.5 天 | `cargo test -p fagus-ffi`；Linux x64 上 `php -r` 產出 `%PDF-` | 完成（`c9971a13`…`cda340c`，見 §14） |
| M2 | release.yml `lib` job、三平台 smoke | 0.5 天 | CI 產出 3 個產物；dumpbin / readelf 檢查通過 | 建置、smoke、封裝與 draft release 已在 `ffi-release.yml` 完成；併入 `release.yml` 待做 |
| M3 | `php/fagus-ffi`（7.4 語法）+ Composer | 1 天 | PHP 7.4 / 8.0 / 8.3 / 8.4 全綠 | 完成（`b1831f0`…`350f7dc`），含 PKCS#12 簽章 |
| M4 | 文件與釘選流程 | 0.5–1 天 | 文件與 release 產物一致 | 文件已完成（`docs/php.md`、`SECURITY.md`、`CHANGELOG.md`）；釘選流程待做 |

約 3–4 天可交出不含 callback 的 MVP。

## 10. 決策（2026-11-20 已定）

1. **PHP 7.4 的 API 範圍：精簡。** 7.4 套件只提供 render 與核心選項（PDF/A、附件、加密、
   Factur-X、字型），不複製 8.3 的完整 `Document` 鏈式 API。與 `nforst/fagus` 分開維護。
2. **Windows 只支援 x64。** 不出 `i686-pc-windows-msvc`；文件與 CI 明確寫死 x64。
3. **PKCS#12 簽章列入第一階段。** 它不需要 callback，只是 `Signature::pkcs12()` 加請求欄位，
   所以 C ABI 直接支援；外部簽章器、時間戳、吊銷查詢仍留給 worker，FFI 明確拒絕而非默默忽略。
4. **`+crt-static` 只套用在函式庫的建置步驟。** 不用全域 `.cargo/config.toml`（它會一起改變
   CLI 的 Windows 產物），而是在 Windows 的 build step 設 `RUSTFLAGS=-C
   target-feature=+crt-static`。`cargo rustc -- -C …` 只作用在最後一個 crate，相依 crate 仍可能
   帶進動態 CRT，spike 驗證的是整棵依賴圖一起套用。

## 11. 驗證清單（實作後）

- [x] `fagus_abi_version()` 與 `fagus.h` 一致
- [x] 所有 `extern "C"` 入口包 `catch_unwind`
- [x] 空指標、長度 0、超大輸入、無效 JSON 都有明確錯誤（無效 JSON 由 `serde_json` 回報；
      null 指標在 `fagus_render` 入口檢查）
- [x] `dumpbin /dependents` 無 `VCRUNTIME140` / `MSVCP140` / `ucrtbase`（spike，§13）
- [x] `readelf --version-info` 最高 `GLIBC_2.17`（spike，§13）
- [x] `nm -D --defined-only` 只有 `fagus_*`（spike，§13）
- [x] PHP 7.4 / 8.0 / 8.3 / 8.4 smoke test 全綠（CI `44732710202`，含 PHP 套件）
- [x] PKCS#12 簽章經 C ABI 產出（`cargo test -p fagus-ffi`、PHP smoke）
- [ ] `php -r 'echo ini_get("ffi.enable");'` 在 FPM 與 CLI 都是 `true`（或 preload）
- [x] FFI 與 CLI 輸出位元組相同：`ffi.yml` 的「The library and the command line agree byte for
      byte」以同一份 HTML 與 `--created` 比對 `cmp`
- [x] 文件標明無沙箱（`docs/php.md`、`SECURITY.md`）

## 12. 校對記錄（2026-11-20）

初稿完成後上網核對，修正了三處事實錯誤，其餘結論不變。

| 項目 | 初稿 | 查證結果 | 處理 |
|---|---|---|---|
| PHP 7.4 的 MSVC | VS16 | **VS15（Visual Studio 2017）** | 已改為 7.2–7.4 = VS15 |
| PHP 8.2/8.3 的 MSVC | VS17 | **VS16（Visual Studio 2019）** | 已改為 8.0–8.3 = VS16 |
| 官方 PHP Docker 映像的 FFI | 假設內建 | **沒有內建**，需 `docker-php-ext-install ffi` | 已補 Dockerfile 與 `libffi-dev` |
| `ffi.enable` 預設值 | `preload` | 正確（INI_SYSTEM） | 不變 |
| Debian glibc | 2.31 / 2.36 / 2.41 | 正確 | 不變 |
| `fontdb` fontconfig | 純 Rust | 正確（`fontconfig-parser`） | 不變 |
| `+crt-static` 用於 cdylib | 可行 | 正確（rust-lang 維護者確認） | 補上 RUSTFLAGS 的注意事項 |
| GoReleaser Rust builder | v2.5+，v2.19 起非實驗 | 正確 | 不變 |

來源：

- PHP Windows build 的 Visual Studio 版本：<https://wiki.php.net/internals/windows/php-windows-builder>
  （7.2–7.4 = VC15、8.0–8.3 = VS16、8.4–8.5 = VS17）
- `ffi.enable` 預設與限制：<https://www.php.net/manual/en/ffi.configuration.php>
- FFI callback 的限制：<https://www.php.net/manual/en/ffi.examples-callback.php>
- FFI 在 Windows 需 `php_ffi.dll`：<https://www.php.net/manual/en/ffi.installation.php>
- `crt-static` 與 cdylib：<https://github.com/rust-lang/rust/issues/71651>
- Debian glibc 版本：<https://tracker.debian.org/pkg/glibc>
- `fontdb` 0.23 的 fontconfig 功能：<https://docs.rs/crate/fontdb/0.23.0/features>
- GoReleaser Rust builder：<https://goreleaser.com/customization/builds/builders/rust>

尚未在真實環境驗證、需要在 M1/M2 實測的假設：

1. ~~PHP 7.4 的 C 宣告解析器能接受 `fagus.h`~~ → 已驗證，見 §13。
2. ~~`+crt-static` 的 DLL 在 PHP 7.4（VS15）與 8.4（VS17）上都能被 `FFI::cdef` 載入~~ → 已驗證。
3. ~~`cargo-zigbuild` 產出的 `libfagus_ffi.so` 只要求 `GLIBC_2.17` 符號~~ → 已驗證。
4. ~~官方 `php:7.4-cli` 映像能成功 `docker-php-ext-install ffi`~~ → 已驗證。

## 13. Spike 結果（2026-11-20）

`spike/ffi` 分支（commit `test(ffi): spike the C ABI shared library build`）推上 GitHub Actions，
workflow `Spike FFI`（`.github/workflows/spike-ffi.yml`）三個 job 全綠：

| Job | 結果 |
|---|---|
| Linux x86_64 | `highest: GLIBC_2.17`；只匯出 `fagus_*`；PHP 7.4 / 8.0 / 8.3 / 8.4 全部 `PASS` |
| Linux aarch64 | `GLIBC_2.17`；PHP 8.4 `PASS`（原生 arm64 runner） |
| Windows x86_64 | PHP 8.4.28 與 7.4.33 都 `PASS`；同一份 DLL |

`dumpbin /dependents fagus_ffi.dll` 只剩系統 DLL：

```
bcryptprimitives.dll
kernel32.dll
api-ms-win-core-synch-l1-2-0.dll
KERNEL32.dll
ntdll.dll
```

沒有 `VCRUNTIME140` / `MSVCP140` / `ucrtbase`，所以 `+crt-static` 達成目標：一份 DLL 同時服務
PHP 7.4（VS15）與 8.4（VS17），不需要使用者安裝 Visual C++ redistributable。

每次 smoke 都真的在 PHP process 內渲染了一份小 PDF（`fagus_spike_probe()` 回傳 4254 bytes），
不只是載入符號；`objdump -T` 也沒有 `GLIBCXX_` / `CXXABI_` 依賴。

### 新發現：PHP 7.4 的 `const char *` 回傳值

PHP 7.4 會把 `const char *` 的回傳值直接轉成 PHP `string`，不是 `FFI\CData`：

```php
// PHP 7.4: $ffi->fagus_version() 是 string，FFI::string() 會丟 TypeError
$version = is_string($v) ? $v : FFI::string($v);
```

PHP 8.x 之後才可能是 `CData`。`php/fagus-ffi` 與 smoke test 都要用上面這種寫法，
不能直接 `FFI::string()`。

### 帶回 plan 的結論

- M1 可以直接用 `crates/fagus-ffi` 目前的最小 ABI 與 `include/fagus.h`，不需要改形狀。
- Windows 的 `+crt-static` 可以定案；正式 build 只對 `fagus-ffi` 套用（`cargo rustc`，見 §10.4），
  不用全域 `RUSTFLAGS`。
- glibc 2.17 的 baseline 成立，Linux 只出 gnu 版即可。
- PHP 端包裝要處理 7.4 的 `const char *` 差異（已列入 §5）。

## 14. 進度與後續（2026-11-20 收尾）

### 已完成

| 項目 | 位置 | 驗證 |
|---|---|---|
| 請求 JSON 與輸出 JSON 共用 | `crates/fagus-protocol` | CLI 測試、`fagus-ffi` 測試 |
| C ABI（engine、render、free、last error） | `crates/fagus-ffi`、`include/fagus.h` | `cargo test -p fagus-ffi`；CI 三平台 |
| 真實渲染與 metadata JSON | `crates/fagus-ffi/src/lib.rs` | PHP smoke 產出 `%PDF-` |
| PKCS#12 簽章 | 同上，`signature()` | `signs_with_pkcs12`；PHP smoke |
| PHP 7.4 套件 | `php/fagus-ffi`（`Engine`/`Document`/`Pdf`/`Signature`/`Diagnostic`） | PHP 7.4、8.0、8.3、8.4（CI `44732710202`） |
| 平台二進位搜尋 | `Native::locate()` 找 `lib/<platform>/`，可用 `FAGUS_FFI_LIB` 覆寫 | 路徑邏輯 |

CI（workflow `Spike FFI`，分支 `spike/ffi`）：

- `fmt, clippy and tests`：`cargo fmt --all --check`、
  `cargo clippy -p fagus-cli -p fagus-protocol -p fagus-ffi --all-targets -- -D warnings`、
  同三個 crate 的 `cargo test`
- `Linux x86_64`：zigbuild `GLIBC_2.17`，PHP 7.4 / 8.0 / 8.3 / 8.4 跑 `php/fagus-ffi/tests/smoke.php`
- `Linux aarch64`：原生 arm64 runner，PHP 8.4
- `Windows x86_64`：`+crt-static` DLL，PHP 7.4 與 8.4

### 本輪新發現

1. **PHP 7.4 不接受 PHP string 傳給 `uint8_t *`**（只接受 `char *`）。spike 只驗證了
   `const char *` 回傳值，沒有驗證輸入。包裝層現在一律
   `FFI::new('uint8_t[n]')` + `FFI::memcpy`，長度 0 傳 `null`。
2. **`layout` 是 `{pages, elements:[{id, tag, boxes}]}`**，不是以 `id` 為鍵的 map。
3. **`+crt-static` 的範圍要收斂**：放進 `.cargo/config.toml` 會連 CLI 的 Windows 產物一起改，
   因此 release job 改用 `cargo rustc -p fagus-ffi ... -- -C target-feature=+crt-static`。
4. 官方 `php:*-cli` 映像沒有 FFI，`php/fagus-ffi/tests/php-smoke.sh` 會安裝 `libffi-dev` 並
   `docker-php-ext-install ffi`，Linux CI 直接重用它。

### 待辦（M2 收尾與 M4）

1. ~~`.github/workflows/release.yml` 新增 `lib` job（三個 target）~~ → 先在獨立的
   `.github/workflows/ffi-release.yml` 驗證（見 §15），確認封裝與 PHP 套件組裝後再併入
   `release.yml`。
2. ~~在 `spike-ffi.yml` 先驗證封裝內容與 artifact 名稱~~ → 改由 §15 的 workflow 直接驗證
   封裝、`lib/<platform>/` 佈局與 draft release。
3. ~~PHP 套件發佈：把三平台二進位放進 `php/fagus-ffi/lib/<platform>/`，或由 Composer 安裝流程
   下載；`Native::locate()` 已支援這個路徑。~~ → `ffi-release.yml` 的 `php` job 已把三個平台
   放進 `package/lib/<platform>/` 並用不含明確路徑的 smoke test 驗證。
4. ~~文件：`docs/php.md`（FFI 用法與限制）、`SECURITY.md`、`CHANGELOG.md`；
   `docs/nav.yml` 與 `php tools/check-docs.php`。~~ → 已寫入（沒有新頁面，`nav.yml` 不需新增）；
   `php tools/check-docs.php` 通過。
5. ~~進 PR 前：CI 恢復 `--locked`、決定 `spike-ffi.yml` 的去留、補一個 `ffi.enable=preload`
   的 FPM 驗證。~~ → 已把 `spike-ffi.yml` 改名為 `ffi.yml`（main / PR 觸發、`--locked`、移除與
   `ci.yml` 重複的 check job）；`ffi.enable=preload` 的 FPM 驗證仍待做。
6. ~~驗證 FFI 與 CLI 在同一份 HTML 與 `--created` 下位元組相同。~~ → `ffi.yml` 的 parity
   步驟會建置 CLI、用 `parity.php` 經 FFI 渲染同一份 HTML，然後 `cmp` 兩份 PDF。

## 15. FFI 預覽發版 workflow（2026-11-20）

`.github/workflows/ffi-release.yml`，只在 `spike/ffi` 上存在，不會影響 `main` 的 `release.yml`。

### 觸發方式

| 觸發 | 行為 |
|---|---|
| push tag `ffi-v*` | 建置、測試、產生 **draft release**（不會自動公開） |
| `workflow_dispatch`（輸入 `version`） | 只建置並上傳 workflow artifact，不建立 release |

版本規則：`ffi-v0.2.0-rc1` → `0.2.0-rc1`；`-` 之前的部分必須等於 `Cargo.toml` 的
`version`，否則 job 直接失敗。

發版前先確認 tag 指向 `spike/ffi`：`release` job 會 `git merge-base --is-ancestor
"$GITHUB_SHA" FETCH_HEAD`，避免有人把 tag 打在別的 commit 上。

```sh
git tag ffi-v0.2.0-rc1
git push origin ffi-v0.2.0-rc1
```

### Job

1. `library`（matrix 三個 target）
   - Linux x86_64 / aarch64：`cargo zigbuild … .2.17`，檢查 `GLIBC_2.17`、只匯出 `fagus_*`、
     無 `GLIBCXX_` / `CXXABI_`
   - Windows x64：`RUSTFLAGS=-C target-feature=+crt-static`（整棵依賴圖），`dumpbin
     /dependents` 不得有 `VCRUNTIME` / `MSVCP` / `ucrtbase` / `MSVCR`
   - PHP smoke：x64 跑 7.4 / 8.0 / 8.3 / 8.4（Docker `php:*-cli`）；arm64 跑 8.4；
     Windows 跑 7.4 與 8.4（`shivammathur/setup-php`）
   - 產物：`fagus-ffi-<target>.tar.gz` 與 `.zip`（函式庫 + `fagus.h` + README），各附
     build provenance attestation
2. `php`
   - 下載三個平台的函式庫，放進 `package/lib/{linux-x86_64,linux-aarch64,windows-x86_64}/`
   - 用**不含明確路徑**的 smoke test 驗證 `Native::locate()` 的封裝佈局
   - 產物：`fagus-ffi-php-<version>.tar.gz`，附 attestation
3. `release`（只在 tag 觸發）
   - 合併所有產物、加上 `THIRD-PARTY-NOTICES`、產生 `SHA256SUMS` 並做 attestation
   - `gh release create "ffi-v<version>" … --draft`：保持 draft，人工檢查後才發布

### 尚未包含

- CycloneDX SBOM（現有 CLI release 有；函式庫可之後補）
- Packagist / Composer 發佈流程；目前只產出 tar.gz
- `tools/pin-binaries.php` 的釘選流程（等併入 `release.yml` 時一起做）
- musl：不列入，PHP 官方映像都是 glibc

## 16. Preview run 44735444601 與收尾（2026-11-21）

第一支 `ffi-v0.2.0-rc1` 的 `FFI preview release` 在 Windows job 的
`actions/attest-build-provenance@v2` 失敗：

```
InternalError: error fetching tlog entry - (404) Not Found
```

建置、`dumpbin`、封裝與 PHP 7.4 / 8.4 smoke 都成功，Linux 兩個 target 與其 attestation 也成功；
失敗發生在 Sigstore 的 Rekor 寫入後回讀（transient，[actions/attest-build-provenance#679]）。
處理方式：

1. 重跑失敗的 job（`gh run rerun 44735444601 --failed`），不需要改動 commit。
2. `ffi-release.yml` 的三個 attestation 步驟（library、php、release）都改成
   `continue-on-error` + 等 60 秒重試一次；只有第二次失敗才讓 job 失敗。Rekor 的讀取通常幾分鐘
   內恢復，重試可避免整個 preview release 卡住。

同一輪的其他收尾：

- `spike-ffi.yml` → `ffi.yml`，在 `main` / PR 上依 FFI 相關路徑觸發；移除與 `ci.yml` 重複的
  check job，建置恢復 `--locked`，cache key 改名。
- `ffi.yml` 新增 parity 步驟：建置 `fagus` CLI，同一份 HTML 與 `--created` 分別經 CLI 與
  `parity.php`（FFI）渲染，`cmp` 兩份 PDF。
- 文件：`docs/php.md` 新增「The FFI package」、`SECURITY.md` 補無沙箱與 abort/OOM 風險、
  `CHANGELOG.md` 的 `[Unreleased]` 補條目。

仍待做：

- `ffi.enable=preload` 的 FPM 驗證（需要 php-fpm + preload 檔）。
- 把 `ffi-release.yml` 的 `library` / `php` job 併入 `release.yml`，以及
  `tools/pin-binaries.php` 的釘選流程。
- SBOM 與 Packagist / Composer 發佈。

重跑後 Windows job（含 attestation）全綠，證實 404 是 transient；接著暴露第二個問題：
`php` job 的 `tar -xzf … --wildcards '*.so' '*.dll'` 在只有 `.so` 的 Linux archive 上
以 exit 2 結束（`tar: *.dll: Not found in archive`）。這是第一次跑到該步驟才出現的錯誤。
修正：依 `platform` 只解開對應的 pattern（Linux 用 `*.so`，Windows 用 `*.dll`），
`fagus.h` 與 README 留在 archive 的 `dist/`，不放進 `lib/<platform>/`。

[actions/attest-build-provenance#679]: https://github.com/actions/attest-build-provenance/issues/679


