# 依頼作成支援エージェント

依頼の登録の画面（`/requests/new`）の右の欄は、利用者の Mac に入っている `claude` の CLI（Claude Code）で動く会話です。
API key の入力も、bantou サーバー経由の呼び出しも、ありません。CLI が login 済みなら、そのまま使えます。
依頼の一覧・状況の画面の右の欄は、これまでどおり「未接続」の枠のままです。

## 何ができるか

- 利用者の発言ごとに、エージェントの返事が出ます。返事は、今まとめている依頼（題名・本文・プロジェクト・種別）を前提にします。
  発言のたびに、そのときの編集中の値を渡します（利用者が直した内容も、次の発言から見えます）。
- 利用者が頼んだ時だけ、エージェントは題名・本文を書き換えます（本文は全面の書き換えも可）。左の編集欄にすぐ入り、会話の欄に
  「題名を書き換えました」「本文を書き換えました」の行が出ます。エージェントが書き換えない限り、利用者が書いた内容は変わりません。
- 返事を待つ間に利用者がその欄（題名・本文）を編集していたら、書き換えは反映しません。会話の欄に
  「編集中のため、エージェントの書き換えを反映しませんでした」の行と「反映する」のボタンが出て、押した時だけ反映します。
  送った時の値から変わっていない欄は、これまでどおり、すぐ反映します。
- 会話は、同じ登録の画面を開いている間、続きます。画面を閉じるか、別の依頼に移ると、新しい会話になります。
  途中保存した依頼の会話の再開はありません。session id は、画面の状態として持つだけです。
- 編集できるのは、今まとめている依頼だけです。依頼が draft でない時と、読み込み中は、送れません。

## 構成

- Rust（`client/src-tauri/src/assist.rs`、命令 `assist_send`）: CLI を探し、起こし、答えを読む。webview には、`answer` か `notice` の 1 つだけを返す（命令は失敗しない）。
- 画面（`client/app/utils/assist.ts`、`RequestsAgentPane.vue`、`pages/requests/new.vue`）: 会話の行、送信中の表示、書き換えの反映。

## CLI の探し方

1. 環境変数 `BINONE_CLAUDE_BIN` が在れば、それを使う（test と実物確認用。file でなければ「見つからない」）。
2. `/opt/homebrew/bin/claude`、`/usr/local/bin/claude`、`~/.local/bin/claude`、`~/.claude/local/claude` の順。
   `~/.local/bin/claude` で見つかる Mac は実在します（候補に含めて問題ありません）。
3. 無ければ `/bin/zsh -lc 'command -v claude'`（10 秒まで）。GUI から起動した app は login shell の PATH を持たないので、この順で探す。

見つからない時は「claude の CLI（Claude Code）が見つかりません。…」の案内を会話の欄に出します。

## 起こし方

1 回の返事ごとに `claude -p` を 1 回起こします。発言は標準入力で渡します（`-` で始まる文や長い文でも引数にならない）。

```
claude -p --output-format json --tools "" --setting-sources "" --disable-slash-commands \
       --strict-mcp-config --system-prompt <指示> [--resume <session id>]
```

- 初回の答え（JSON の `session_id`）を控え、2 回目から `--resume <id>` で続けます。`--no-session-persistence` は付けません（付けると session が残らず、`--resume` で続けられなくなる）。
- 1 回の返事は 120 秒まで。超えたら、起こした process とその子を止めて、案内を出します。
- 返事を待つ間、送信ボタンは押せず、「返事を待っています…」を出します。

## 会話の記録

会話の記録は、利用者自身の claude の session として、`~/.claude/projects/` の下の、app 専用の cwd の名前から作られた dir に、1 会話につき 1 file 残ります
（`--no-session-persistence` を付けないため）。app はこれを消しません。利用者の claude の履歴に、この app の会話が混ざる事になります。

## 渡す物・渡さない物

| 渡す | 渡さない |
|---|---|
| 利用者の発言 | 利用者の file、他の依頼 |
| 今の依頼の題名・本文・プロジェクト・種別（毎回、そのときの値） | タグ・protocol・server の address と key |
| 答えの形を決める system prompt | 利用者の `CLAUDE.md`、project の設定、skill、MCP server |

## 無効にした物

- 組み込みの tool は全て（`--tools ""`）。file の読み書き・command の実行・web の取得は、できません。
- 設定の読み込み（`--setting-sources ""`: user・project・local のどれも読まない）。`""` が「何も読まない」になる事は、Claude Code の文書と実機で確かめた範囲で、そう扱っています（それ以上は保証しません）。slash command と skill（`--disable-slash-commands`）。MCP server（`--strict-mcp-config`、設定なし）。
- cwd は、この app 専用の空の dir（app の data dir の下の `assist-cwd`）。上の階層の `CLAUDE.md` や project の設定を拾いません。
- login の情報（OAuth・keychain）は CLI 自身が使います。`--bare` は、それを使えなくするので付けません。

## 答えの形

system prompt で、次の JSON オブジェクト 1 つだけを返すよう指示します。

```json
{"reply": "利用者への返事", "title": "題名の新しい値", "body": "本文の新しい値"}
```

`reply` は必ず入れる。`title` と `body` は書き換える時だけ入れる。``` の囲みや前後の文が付いても、最初の `{` から最後の `}` までを読みます。
形が崩れた時（JSON でない、`reply` が無い）は、答えの文をそのまま返事として出します。書き換えは起きません。

## うまくいかない時

どれも会話の欄に日本語の 1 行と、CLI の出力の要点（末尾 400 字まで）を出します。app は落ちず、固まりません。

| 場合 | 案内 |
|---|---|
| CLI が無い | 「claude の CLI（Claude Code）が見つかりません。…」 |
| login していない（終了 code が 0 でなく、出力に "not logged in" か "/login" が在る時だけ） | 「claude にログインしていません。ターミナルで claude を起動して /login を実行してから…」 |
| 終了 code が 0 でない、`is_error` の答え（API error など）。"authentication"・"api key" だけの出力もここ | 「エージェントの呼び出しに失敗しました。」と、CLI の出力の要点（末尾 400 字） |
| 120 秒を超えた | 「返事が 120 秒たっても返らなかったので、待つのをやめました。…」 |

## test

- Rust: `BINONE_CLAUDE_BIN` が指す先と同じ役目の、偽の実行 file（shell script）で、正常・題名だけ・本文・形の崩れ・非 0 終了・login なし・時間切れ・見つからない・探す順・渡す引数を確かめる。本物の `claude` は呼ばない。
- 画面: `client/tests/assist.test.ts`（送信中、返事、書き換えと「書き換えました」の行、案内、見本 data）。
