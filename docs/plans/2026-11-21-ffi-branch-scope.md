# 規劃：FFI 分支的範圍：保留、分包，還是移除 CLI 與其他 Composer 套件

狀態：已決策：方案 A（2026-11-21），保留 CLI 與其他 Composer 套件，FFI 只新增
日期：2026-11-21
範圍：`spike/ffi` 分支的檔案邊界、Composer 發佈邊界、CI 與文件
非範圍：C ABI 或 PHP API 的重新設計、worker 協定的變更、實際刪除檔案

## 1. 問題

`spike/ffi` 的目的是 C ABI 動態庫（`crates/fagus-ffi`）與 PHP 7.4 套件
（`php/fagus-ffi`）。但這個 repo 同時是：

- Rust workspace：引擎、CLI、worker 協定、FFI；
- Composer 套件 `nforst/fagus`：worker 模式、PHP 8.3+、Laravel 整合；
- 文件、測試語料、WPT 與 Chrome 比對工具。

問題是：**既然這個分支只專注 PHP FFI wrapper，其他 Composer 套件（`php/fagus`、
`php/laravel`、root `composer.json`）與 CLI（`crates/fagus-cli`）是否應該一併清掉？**

先區分兩種「清理」，後面的方案都建立在這個區分上：

1. **移除原始碼**：把檔案從分支刪掉，CI、文件、Cargo workspace 一起縮小。
2. **只縮小發佈邊界**：原始碼留著，但 `nforst/fagus-ffi` 的 archive 或獨立 repo 不含
   其他套件與 CLI。

兩者的成本與風險差很多，不能混為一談。

## 2. 現況與依賴

| 事實 | 位置 | 對「清理」的意義 |
|---|---|---|
| FFI 依賴 `fagus-protocol` | `crates/fagus-ffi/Cargo.toml` | `fagus-protocol` 不能刪 |
| `fagus-protocol` 也被 worker 使用 | `crates/fagus-cli/src/worker.rs` | 刪 CLI 後它仍有 FFI 這個使用者 |
| 引擎本身獨立 | `crates/fagus` | 一定要留 |
| CLI 是 worker 的宿主與沙箱 | `crates/fagus-cli/src/{worker,sandbox,memory}.rs` | 刪了就沒有 Landlock/seccomp 路徑 |
| root `composer.json` 是 `nforst/fagus` | `composer.json` | 刪了就不能發佈 worker 套件 |
| `php/fagus`、`php/laravel` 是 root 套件的內容 | `php/fagus`（36 檔）、`php/laravel`（14 檔） | 與 FFI 套件無程式依賴 |
| `php/fagus-ffi` 有自己的 `composer.json` | `nforst/fagus-ffi`，PHP >= 7.4 | 已經是獨立套件，只是放在子目錄 |
| parity 測試以 CLI 為基準 | `.github/workflows/ffi.yml`、`php/fagus-ffi/tests/parity.php` | 刪 CLI 就失去「FFI 與 CLI 位元組相同」的驗收 |
| `SECURITY.md` 的安全承諾依賴 worker 沙箱 | `SECURITY.md` | 刪 CLI 後只剩「FFI 無沙箱」 |
| `.gitattributes` 已把 Rust、tests、tools、docs 排除在 root archive 外 | `.gitattributes` | 這是 root 套件的邊界，不是 FFI 套件的 |
| 文件有 `cli`、`protocol`、`php`、`laravel`、`deployment` 頁 | `docs/nav.yml` | 刪除會產生大量斷鏈與導覽變更 |
| CI 有 PHP 8.3/8.5、Laravel、worker 的 job | `.github/workflows/ci.yml` | 刪除可縮短 CI，但要同步移除 |
| fuzz 有 `worker` target | `fuzz/fuzz_targets/worker.rs` | 刪 CLI 後要改 target |

結論：**FFI 套件對其他 Composer 套件沒有依賴，但反過來，CLI 與 worker 套件承載了安全承諾與
parity 基準。** 所以「能不能刪」不是編譯問題，而是產品與驗收問題。

## 3. 方案

### 方案 A：全部保留，FFI 只是新增（現狀）

分支照原計畫併回 `main`。`nforst/fagus`、Laravel 整合、CLI、worker 都留著。

- 優點：不破壞既有產品；沙箱與 parity 基準都在；merge 衝突最少。
- 缺點：FFI 的 review 與 CI 仍要背著無關的 PHP/Laravel 變更。
- 適用：`spike/ffi` 最終要併回 `main`。

### 方案 B：只縮小發佈邊界（建議）

原始碼全部保留，但把 `nforst/fagus-ffi` 當成獨立發佈物：

1. 在 `php/fagus-ffi/` 加上自己的 `.gitattributes`，排除 `tests/`（若不想隨套件發佈）與
   任何開發檔。
2. 用 Composer 的 subtree split（或 GitHub 上的 split repo）把 `php/fagus-ffi/` 變成
   `nforst/fagus-ffi` 的套件根目錄。
3. 三平台二進位原本由 `ffi-release.yml` 放進 `package/lib/<platform>/`，這條路不變。
4. root `.gitattributes` 維持現狀，`nforst/fagus` 的 archive 仍然不含 Rust 與 FFI 套件。

- 優點：達到「FFI 套件乾淨」的目標，又不刪任何產品程式；parity 與沙箱都留著。
- 缺點：需要一個 split 流程（CI 或手動）；兩個套件的版本與 release 要分開管理。
- 適用：同一 repo 同時發佈 worker 套件與 FFI 套件。

### 方案 C：在這個分支移除 CLI 與其他 Composer 套件

只留引擎、`fagus-protocol`、`fagus-ffi`、`php/fagus-ffi` 與 FFI 的文件/CI。

需要一起處理的檔案與設定：

- Cargo：`crates/fagus-cli`、workspace member、`fuzz` 的 `worker` target、
  `tools/worker-memory.py`、依賴 CLI 的 tools 腳本。
- Composer：root `composer.json`、`phpunit.xml.dist`、`pint.json`、`php/fagus`、
  `php/laravel`、`php/fagus/bin/fagus-install`。
- CI：`ci.yml` 的 php、sandbox、fuzz job；`ffi.yml` 的 parity 步驟要換成 golden PDF。
- 文件：`docs/cli.md`、`docs/protocol.md`、`docs/php.md`、`docs/laravel.md`、
  `docs/deployment.md`、`docs/nav.yml` 與所有內部連結。
- 安全：`SECURITY.md` 的沙箱承諾改成「只有 FFI，無沙箱」。
- 發佈：`release.yml`、「`tools/pin-binaries.php`」、`docs/releasing.md` 的 binary 流程。
- 測試：PHP 7.4/8.0/8.3/8.4 的 smoke 仍可留；parity 需要新的基準。

- 優點：分支與 CI 真的只剩 FFI；review 面積最小。
- 缺點：**失去唯一的沙箱路徑**（`SECURITY.md` 目前建議不可信 HTML 走 worker）；
  失去 CLI 這個 parity 基準；`nforst/fagus` 不能再從這個分支發佈；併回 `main` 會是
  大規模刪除，而不是新增功能。
- 適用：這個 repo 的產品線確定只剩 FFI，worker 套件移到別處或停止維護。

### 方案 D：拆成兩個 repo

- `nforst/fagus`：引擎 + CLI + worker 套件。
- `nforst/fagus-ffi`：FFI crate + `php/fagus-ffi`，以 git submodule 或 subtree 指回引擎。

- 優點：邊界最清楚，各自的 CI 與 release 獨立。
- 缺點：引擎改動要跨 repo 同步；版本與 binary 釘選更複雜；對 0.2.x 的專案來說太重。
- 適用：FFI 套件要長期獨立於 worker 套件演進。

## 4. 建議

1. **如果 `spike/ffi` 最終要併回 `main`（目前計畫如此）：選方案 A。**
   這個分支的價值是新增 C ABI，不是取代 worker；現在刪 CLI 會讓 `main` 的產品與安全承諾
   一起消失。
2. **如果目標只是「不要讓 FFI 套件帶著其他 Composer 程式」：選方案 B。**
   用發佈邊界解決，不要用刪除原始碼解決。
3. **只有在確認 `nforst/fagus` 與 worker 不再從這個 repo 發佈時，才考慮 C 或 D。**
   那應該是另一個 repo/分支的決策，不建議在 `spike/ffi` 上做。

## 5. 待確認（review 時請回答）

1. `spike/ffi` 最終要 merge 回 `main`，還是獨立成產品分支？
2. `nforst/fagus`（worker）與 Laravel 整合是否繼續維護與發佈？
3. `nforst/fagus-ffi` 要用 subtree split、獨立 repo，還是暫時只發 tar.gz？
4. 若移除 CLI，parity 驗收是否接受 golden PDF fixtures（固定 HTML + 固定 `--created` 的
   雜湊）取代 CLI 基準？

## 6. 若選 C/D 的粗略順序（僅供評估，不在本輪實施）

1. 先切出獨立 repo 或確認 worker 套件的去處。
2. 再移除 CLI/Composer 檔案，同步 Cargo workspace、fuzz、CI、tools。
3. 改寫 `SECURITY.md`、`docs/` 與 `docs/nav.yml`，跑 `php tools/check-docs.php`。
4. 以 golden PDF 取代 parity，更新 `ffi.yml` 與 `ffi-release.yml`。
5. 最後才刪除 `release.yml` 與釘選流程。

估計：方案 B 約半天（split 流程 + 文件），方案 C 約 2–3 天（刪除、CI、文件、替代驗收），
方案 D 約 3–5 天（跨 repo 同步與發佈）。

## 7. 決策（2026-11-21）：方案 A

選擇方案 A：**CLI、worker 套件與 Laravel 整合全部保留，FFI 只新增，不清理。**

理由：

- `spike/ffi` 最終要併回 `main`，而 `main` 的產品是 `nforst/fagus` + worker + Laravel；
  在這個分支刪除它們會變成「merge 時刪掉既有產品」，不是新增功能。
- CLI/worker 是目前唯一有沙箱的路徑，也是 `SECURITY.md` 對不可信 HTML 的建議做法。
- CLI 同時是 FFI↔CLI byte parity 的基準；保留它才能繼續用現在的驗收。
- `php/fagus-ffi` 本來就是獨立套件（自己的 `composer.json`、`lib/<platform>/` 佈局），
  不需要靠刪除其他程式來保持乾淨。

因此本規劃**沒有要實作的項目**：不移除檔案、不改 Cargo workspace、不改 `composer.json`、
不改 CI 的 PHP/Laravel job。方案 B（subtree split）與 C/D 留作未來選項，只有當
`nforst/fagus` 不再從這個 repo 發佈時才重新評估。

後續工作回到 `docs/plans/2026-11-20-php-ffi-cdylib.md` §14 / §16 的待辦：FPM
`ffi.enable=preload` 驗證、把 `ffi-release.yml` 併入 `release.yml`、SBOM 與 Composer 發佈
（前三項已於該檔 §18 完成，只剩 Packagist 發佈）。

