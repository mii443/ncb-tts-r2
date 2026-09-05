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

参考: [Rust Cacheの動作](https://github.com/Swatinem/rust-cache#cache-details)、[DockerのActionsキャッシュ](https://docs.docker.com/build/cache/backends/gha/)、[GitHubのキャッシュアクセス範囲](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching#restrictions-for-accessing-a-cache)。
