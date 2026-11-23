[English](README.md) | [한국어](README.ko.md) | **日本語** | [简体中文](README.zh-CN.md)

# bmo-voice

音声エンジンを、独立した一つのサービスとして提供します。

- **ボイス:** 学習済みのボイスを名前で管理します。各ボイスは参照クリップなしで話す Fish S2-Pro アダプタです。
  退役した VoxCPM2 のボイスはデータとしてのみ残し、配信はしません。
- **音声合成:** テキストをいずれかのボイスノード上で学習済みボイスに読ませ、音声は回答の直後に続きます。
- **文字起こしとエンコード:** 音声ファイルを一つの言語で書き起こし、参照録音を音声として書き出します。
- **スクリプトの読み:** エンジン独自の読み上げルールで、テキストを発話可能な形に整えます。
- **ボイスサーバー:** すべてのボイスノードで Fish S2-Pro サーバーが動き、ボイスのアダプタを読み込む重みに統合して
  使います。カードの順番はカード arbiter 自身のクライアントを通じて受け取り、インストールと更新はこのリポジトリの
  ノードスクリプトが行います。
- **ボイス学習:** 学習に使うセリフをエージェントの話し方で書き、学習ステージを空いているカードに割り当て、学習済み
  ボイスを検査してからインストールします。
- **スクリプトスタジオ:** 元素材からボイス学習用のセリフを録音する画面とサーバーです。韓国語の言語パックもこの
  リポジトリにあります。
- **ブリーフィング:** ルームに向けて話した一行を、そのルーム・セリフ・時刻とともに保存し、イベントストリームで
  通知してリスナーに再生させます。いつ何を話すかは呼び出し側が決めます。
- **ハーネスプラグイン:** `bmo-voice mcp` MCP サーバーと `bmo-voice` スキルです。
- **ホストツール:** ツールホスト上の `voice/servers` です。エンジンのポートに直接触れないセッション向けに、どの
  ボイスノードが登録されているか、そのサーバーが応答するかを返します。

他のユニットとはすべて標準 API だけでやり取りし(`docs/boundary-api.md`)、他のリポジトリのうち依存してよいのは
基盤ライブラリの `bmo-chat`、`bmo-chat-client`、`bmo-ui`、`bmo-text` だけです。

## モデル

リポジトリに重みは含まれていません。音声ノードには次のものが必要です。

| 何 | 入手先 | ライセンス |
|---|---|---|
| Fish Audio S2-Pro のベース重み | `synth/engine.toml` が固定するリビジョンの [`fishaudio/s2-pro`](https://huggingface.co/fishaudio/s2-pro)。`setup` が各ノードへ配ります | Fish Audio Research License：研究・非商用のみ |
| エンジンのコード fish-speech | `synth/engine.toml` が固定するコミットの [`fishaudio/fish-speech`](https://github.com/fishaudio/fish-speech) | 独自のライセンス |
| 文字起こしと声の測定に使う Whisper | [`openai/whisper-large-v3-turbo`](https://huggingface.co/openai/whisper-large-v3-turbo) | MIT |
| 学習済みの声 | このリポジトリのスクリプトで自分のエージェントの録音から学習する、または BMO の声 [`ahas-io/bmo-voice`](https://huggingface.co/ahas-io/bmo-voice) を voices フォルダの下にコピー | ベースと同じ：Fish Audio Research License |

**Built with Fish Audio.** このリポジトリのコードは MIT ですが、S2-Pro の重みとその上で学習した声は MIT ではありません。有料サービス、企業の社内業務、ホスティングされた API で声を使うには、先に Fish Audio のライセンスが必要です（`docs/voice-plan.md`）。

## フォルダ

| フォルダ | 内容 |
|---|---|
| `data/` | コードが言語ごとに読む言語データ(`data/<language>/`)。韓国語の行はすべて `korean-ledger.json` に記録されています |
| `deploy/` | テストベッドのデプロイが管理するユニット(音声サービスとスクリプトスタジオ) |
| `docs/` | コードより先にあるドキュメント。まず `docs/layout.md` から読んでください |
| `plugin/` | ハーネスプラグインのスキル、`plugin/skills/bmo-voice/` |
| `policy/` | エンジン独自のポリシー値、`policy/voice.toml` |
| `prompts/` | エンジン自身のモデル実行に与える指示。学習セリフのライター用で、英語で書かれています |
| `scripts/` | ノード・学習用スクリプト。独立した Bun プロジェクトで、ボイスノードのインストール・プッシュ・同期・ヘルスチェック・発話・カードアダプタ、ボイスのインストールと検査、ボイス名、カードに割り当てる学習ステップ、学習セリフの作成を含みます |
| `src/` | クレート。設定、ストア、イベントログ、クライアント、MCP サーバー、OpenAPI ドキュメント、スクリプトルール、ボイスノード、ルート、ドキュメントの読み上げ、スクリプトスタジオのサーバー、そして `voice-server`・`bmo-voice`・`script-studio` バイナリ |
| `studio/` | スクリプトスタジオのメディアツール。Python スクリプトを併設した Bun プロジェクト(`studio/lib/`、`studio/scripts/`、`studio/fixtures/`)で、画面の `studio/client/` は bmo-ui ベースの Nuxt プロジェクトです |
| `synth/` | 実際に話す部分。Fish S2-Pro の発話器とボイスサーバー、エンジンの固定インストール(`engine.toml`)、エンコーダ、文字起こし器、明瞭度の測定、ツールエントリとそのスキーマ、音声環境の固定パッケージ一覧 |
| `tests/` | このリポジトリのクレートとノードスクリプトのすべてのテスト(言語を問わず) |
| `tools/` | ボイスのホストツール。それぞれマニフェスト・スキーマ・テストを持つワークスペースのクレートで、`tools/servers/` はセッションがボイスサーバーを読むための `voice/servers` エントリです |
| `weights/` | ボイスの重みに関する宣言。ボイスサーバーのカード宣言と、ボイスおよびベースモデルの置き場所。重みそのものは含みません |

## テスト

テストはコードの隣ではなく `tests/` に置きます。ホストツールのテストは各フォルダの `tests/` に、スタジオ画面の
テストは `studio/client/tests/` に置き、後者はそのプロジェクト自身のチェックが型チェックの後に実行します。ノード
スクリプトのテスト `tests/*.test.ts` はリポジトリのルートから `bun test` で実行し、スクリプトと一緒に型チェック
されます。テストベッドの `personal-data` チェックが個人データのルール(`docs/PERSONAL-DATA.md`)を守ります。
追跡対象のファイルには、メールアドレス、ホームパス、電話番号、トークン、プライベートネットワークのアドレスを
含めません。テストが守るべき振る舞いは `docs/inventory.md` に記録されています。

Python のテスト(`tests/*.py`)は専用のインタプリタ `tests/.venv` で動きます。宣言の setup が
`tests/python-requirements.txt` から `uv` でこの環境を作り、テストが読むパッケージは音声環境と同じバージョンに
揃えます。そのため、スイートがどのマシンに配置されても動きます。別のインタプリタを使う場合は
`BMO_VOICE_PYTHON` で指定します。

## 個人データ

リポジトリのどのファイルにも個人データは含まれません。値はインストール先の設定に置き、例には中立的な名前を
使い、`docs/PERSONAL-DATA.md` が定めるルールはテストベッドの `personal-data` チェックが守ります。

## 変更のチェック

- `cargo check --workspace --all-targets`: クレートとホストツールの静的チェック。
- `bmo-testbed check personal-data`: 個人データのチェック。
- `scripts/` で `bunx tsc --noEmit`: スクリプトの静的チェック。
- `studio/client/` で `bun run check`: スタジオ画面の静的チェックと、そのテスト。
- `studio/` で `bunx tsc --noEmit`、および `python3 -m py_compile studio/scripts/*.py`: スタジオツールの静的
  チェック。
- `cargo fmt --all --check` と `cargo clippy --workspace --all-targets -- -D warnings`: レビュー段階。

## バージョン

バージョンは `VERSION` とルートマニフェストの workspace に記載され、両者は同じ番号です。
