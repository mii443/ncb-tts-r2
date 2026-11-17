# GitHub ActionsのRustビルドキャッシュ

## キャッシュの役割

- CIの`rust`ジョブは`Swatinem/rust-cache`で依存crateを保存します。キーにはRustコンパイラ、`Cargo.toml`のfeature設定、`Cargo.lock`、コンパイラ関連環境変数が含まれます。アプリ本体やincremental成果物は保存せず、更新に追従しない巨大なキャッシュを避けます。書き込みは`master`に限定します。
- Dockerは`cargo-chef`の依存レイヤーをBuildKitのキャッシュとして保存します。production用と`--all-targets`のテスト用を両方事前ビルドし、その後にアプリのソースをコピーします。テスト時にだけ有効になる`tokio/test-util`などが、本体変更のたびに大きな依存crateを再コンパイルさせることを防ぎます。
- CIのコンテナビルドは`type=gha,scope=ncb-tts-r2-ci`へ保存します。タグのリリースビルドもこのキャッシュを読み込み、既存のGHCR `:buildcache`もフォールバックとして読み書きします。タグ間で直接共有できないActionsキャッシュを、GHCRのキャッシュで補います。
- ホスト側のdebug成果物とコンテナ内のrelease成果物は、プロファイル・パス・OSが異なるため別々に管理します。テストやClippyをキャッシュ命中でスキップする設定にはしていません。

## 確認方法

1. 初回は新しいRustキャッシュと、テスト用依存を含むDockerレイヤーを作成します。この実行自体は速くならない場合があります。
2. そのCIが成功した後に再実行し、Rust Cacheの`full match: true`、Dockerの`cargo chef cook`レイヤーの`CACHED`を確認します。
3. ソースのみを変更したビルドでは、`cargo test` / `cargo build`の実行は必要ですが、依存関係やfeature設定が同じなら大きな依存crateの再コンパイルは不要です。完全に同じソースの再実行ではアプリのDockerレイヤーも命中するため、ソース変更時の所要時間とは区別します。
4. リリースログでは`gha`と`ghcr.io/mii443/ncb-tts-r2:buildcache`の両方がインポート対象になっていることを確認します。修正前のタグは当時のworkflowを使うため、新しい設定には切り替わりません。

`Cargo.toml`や`Cargo.lock`の変更、コンパイラの変更、キャッシュの期限切れでは再ビルドが必要です。既存キャッシュの削除やビルダーのpruneは不要です。

## 実測（2026-09-06 JST）

修正commit `f53a989`をGitHub-hosted `ubuntu-24.04`で実行しました。[初回](https://github.com/mii443/ncb-tts-r2/actions/runs/33975292187/attempts/1)と[同一commitの再実行](https://github.com/mii443/ncb-tts-r2/actions/runs/33975292187/attempts/2)は、どちらも全ジョブ成功です。時間はジョブの開始から終了までで、キュー待ちは含みません。

| ジョブ | 新キャッシュ作成時 | 同一commitのキャッシュ利用時 |
| --- | ---: | ---: |
| Rust | 5分12秒 | 1分13秒 |
| Docker | 8分40秒 | 24秒 |

- Rustの再実行では`full match: true`を確認しました。コンパイル対象はアプリ本体だけで、Clippyのコンパイルは11.73秒、テストのコンパイルは19.38秒です。通常テスト・doctest・一時UnixソケットのRedis連携テストも通過しました。
- Rustキャッシュは870,718,770 bytes（約0.87GB）です。変更前の直近キャッシュ1,522,142,166 bytes（約1.52GB）と比べて約43%小さくなりました。
- Dockerの再実行では、`cargo chef cook`・`cargo test`・`cargo build`のレイヤーがすべて`CACHED`でした。初回もソースコピー後のテスト・本番ビルドでは外部依存crateの再コンパイルはなく、アプリ本体だけをビルドしています。
- [変更前の実行](https://github.com/mii443/ncb-tts-r2/actions/runs/33973474694)では、Dockerのテスト段階でTokio・Serenity・Songbirdを再コンパイルし、コンパイルに2分17秒かかっていました。修正後の初回では、その依存ビルドがキャッシュ可能な`cargo chef cook`段階に移り、テストのコンパイルは42.59秒でした。

同一commitのDocker 24秒は、テスト・本体のレイヤーも再利用できた場合の値です。ソースが変わる場合はアプリ本体のビルドとテスト実行が必要です。また、単発の計測なのでランナーの性能や転送時間による変動があります。新しいリリースタグの発行・イメージ公開はこの検証では行っていません。

参考: [Rust Cacheの動作](https://github.com/Swatinem/rust-cache#cache-details)、[DockerのActionsキャッシュ](https://docs.docker.com/build/cache/backends/gha/)、[GitHubのキャッシュアクセス範囲](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching#restrictions-for-accessing-a-cache)。
