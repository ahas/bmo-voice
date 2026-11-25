[English](README.md) | [한국어](README.ko.md) | [日本語](README.ja.md) | **简体中文**

# bmo-voice

语音引擎，作为一个独立的服务提供：

- **音色：** 按名称保存训练好的音色，每个音色都是一个无需参考音频即可说话的 Fish S2-Pro 适配器；已退役的 VoxCPM2
  音色仅作为数据保留，不再提供服务。
- **合成：** 在某个语音节点上用训练好的音色朗读文本，声音紧随回答之后播出。
- **转写与编码：** 将语音文件按一种语言转写成文字，并把参考录音写出为声音。
- **脚本朗读：** 按引擎自身的朗读规则把文本整理成可朗读的形式。
- **语音服务器：** 每个语音节点上运行一个 Fish S2-Pro 服务器，把音色的适配器合并进它加载的权重中使用；它通过卡
  arbiter 自带的客户端领取显卡轮次，由本仓库的节点脚本负责安装和更新。
- **音色训练：** 以智能体的说话风格编写训练台词，把训练阶段经由显卡 arbiter 的队列安排到显卡上，检查训练好的音色后再安装。
- **脚本工作室：** 从原始素材录制训练台词的界面和服务器，其韩语语言包也在本仓库中。
- **播报：** 为某个房间说出的一句话，连同房间、台词和时刻一起保存，并通过事件流通知听众播放；何时播报、播报什么由
  调用方决定。
- **Harness 插件：** `bmo-voice mcp` MCP 服务器和 `bmo-voice` 技能。
- **托管工具：** 工具主机上的 `voice/servers`，供不直接访问引擎端口的会话使用，回答登记了哪些语音节点以及它们的
  服务器是否有响应。

与其他所有单元只通过标准 API 通信（`docs/boundary-api.md`）；在其他仓库中，只允许依赖基础库 `bmo-chat`、
`bmo-chat-client`、`bmo-ui` 和 `bmo-text`。

## 模型

仓库中不包含权重。语音节点需要：

| 内容 | 来源 | 许可证 |
|---|---|---|
| Fish Audio S2-Pro 基础权重 | `synth/engine.toml` 固定版本的 [`fishaudio/s2-pro`](https://huggingface.co/fishaudio/s2-pro)，由 `setup` 分发到各节点 | Fish Audio Research License：仅限研究与非商业用途 |
| 引擎代码 fish-speech | `synth/engine.toml` 固定提交的 [`fishaudio/fish-speech`](https://github.com/fishaudio/fish-speech) | 其自身的许可证 |
| 用于转写和声音评测的 Whisper | [`openai/whisper-large-v3-turbo`](https://huggingface.co/openai/whisper-large-v3-turbo) | MIT |
| 训练好的声音 | 用本仓库的脚本以自己智能体的录音训练，或把 BMO 的声音 [`ahas-io/bmo-voice`](https://huggingface.co/ahas-io/bmo-voice) 复制到 voices 文件夹下 | 与基础模型相同：Fish Audio Research License |

**Built with Fish Audio.** 本仓库代码采用 MIT 许可，但 S2-Pro 权重及在其上训练的声音不是。若要在付费服务、企业内部业务或托管 API 中使用这些声音，须先取得 Fish Audio 的许可（见 `docs/voice-plan.md`）。

## 目录

| 目录 | 内容 |
|---|---|
| `data/` | 代码按语言读取的语言资料，`data/<language>/`；其中每一行韩语都记录在 `korean-ledger.json` 中 |
| `deploy/` | 测试台部署所维护的单元（语音服务和脚本工作室） |
| `docs/` | 先于代码的文档：从 `docs/layout.md` 开始阅读 |
| `plugin/` | Harness 插件的技能，`plugin/skills/bmo-voice/` |
| `policy/` | 引擎自身的策略值，`policy/voice.toml` |
| `prompts/` | 引擎自己运行模型时给出的指令：训练台词的撰写者，使用英文 |
| `scripts/` | 节点与训练脚本，一个独立的 Bun 项目：语音节点的安装、推送、同步、健康检查、发声和显卡适配器，音色的安装与检查，音色名称，安排到显卡上的训练步骤，以及训练台词的编写 |
| `src/` | crate：设置、存储、事件日志、客户端、MCP 服务器、OpenAPI 文档、脚本规则、语音节点、路由、文档朗读、脚本工作室的服务器，以及 `voice-server`、`bmo-voice` 和 `script-studio` 三个二进制 |
| `studio/` | 脚本工作室的媒体工具，一个附带 Python 脚本的 Bun 项目（`studio/lib/`、`studio/scripts/`、`studio/fixtures/`），其界面 `studio/client/` 是基于 bmo-ui 的 Nuxt 项目 |
| `synth/` | 负责发声的部分：Fish S2-Pro 发声器和语音服务器、引擎的固定安装（`engine.toml`）、编码器、转写器、清晰度测量、工具入口及其 schema，以及语音环境的冻结包列表 |
| `tests/` | 本仓库 crate 和节点脚本的全部测试，不分语言 |
| `tools/` | 语音的托管工具，每个都是带有清单、schema 和测试的工作区 crate：`tools/servers/` 是会话读取语音服务器状态的 `voice/servers` 入口 |
| `weights/` | 语音权重的声明：语音服务器的显卡声明，以及音色和基础模型的存放位置；不包含权重本身 |

## 测试

测试放在 `tests/` 中，而不是代码旁边；托管工具的测试放在各自目录的 `tests/` 中，工作室界面的测试放在
`studio/client/tests/`，由该项目自身的检查在类型检查之后运行。节点脚本的测试 `tests/*.test.ts` 在仓库根目录用
`bun test` 运行，并与脚本一起做类型检查。测试台的 `personal-data` 检查负责执行个人数据规则
（`docs/PERSONAL-DATA.md`）：受版本控制的文件中不得出现邮箱地址、主目录路径、电话号码、令牌或私有网络地址。
测试需要保持的行为记录在 `docs/inventory.md` 中。

Python 测试（`tests/*.py`）运行在专用解释器 `tests/.venv` 上，由声明中的 setup 用 `uv` 根据
`tests/python-requirements.txt` 创建，测试读取的包与语音环境使用相同版本，因此无论测试套件被安排到哪台机器上都能
运行；也可以用 `BMO_VOICE_PYTHON` 指定其他解释器。

## 个人数据

仓库中的任何文件都不包含个人数据。具体值属于安装环境的设置，示例使用中性名称，`docs/PERSONAL-DATA.md` 规定的规则
由测试台的 `personal-data` 检查执行。

## 检查改动

- `cargo check --workspace --all-targets`：crate 和托管工具的静态检查。
- `bmo-testbed check personal-data`：个人数据检查。
- 在 `scripts/` 中运行 `bunx tsc --noEmit`：脚本的静态检查。
- 在 `studio/client/` 中运行 `bun run check`：工作室界面的静态检查及其测试。
- 在 `studio/` 中运行 `bunx tsc --noEmit`，以及 `python3 -m py_compile studio/scripts/*.py`：工作室工具的静态检查。
- `cargo fmt --all --check` 和 `cargo clippy --workspace --all-targets -- -D warnings`：评审阶段。

## 版本

版本是整个套件的版本，在构建时从 git 标签读取；根清单 workspace 中的版本是从不变动的占位值
(`docs/boundary-api.md`，「Versions」)。
