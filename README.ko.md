[English](README.md) | **한국어** | [日本語](README.ja.md) | [简体中文](README.zh-CN.md)

# bmo-voice

음성 엔진을 하나의 독립된 서비스로 제공합니다.

- **음성:** 학습된 음성을 이름으로 보관합니다. 각 음성은 참조 클립 없이 말하는 Fish S2-Pro 어댑터이며,
  더 이상 쓰지 않는 VoxCPM2 음성은 데이터로만 남겨 두고 서빙하지 않습니다.
- **합성:** 텍스트를 음성 노드 중 하나에서 학습된 음성으로 읽어 주며, 소리는 답변이 나오는 즉시 뒤따릅니다.
- **전사와 인코딩:** 음성 파일을 한 언어로 받아 적고, 참조 녹음을 소리로 기록합니다.
- **스크립트 읽기:** 엔진 자체의 읽기 규칙에 따라 텍스트를 말할 수 있는 형태로 바꿉니다.
- **음성 서버:** 모든 음성 노드에서 Fish S2-Pro 서버가 돌며, 음성의 어댑터를 불러오는 가중치에 병합해
  사용합니다. 카드 차례는 카드 arbiter 자체 클라이언트를 통해 받고, 이 저장소의 노드 스크립트로 설치와
  업데이트를 합니다.
- **음성 학습:** 음성 학습에 쓰는 대사를 에이전트의 말투로 작성하고, 학습 단계를 카드 arbiter의 대기열을 거쳐 카드에 배치하며,
  학습된 음성을 검사한 뒤 설치합니다.
- **스크립트 스튜디오:** 원본 자료에서 음성 학습용 대사를 녹음하는 화면과 서버입니다. 한국어 언어 팩도 이
  저장소에 들어 있습니다.
- **브리핑:** 방(room)을 위해 말한 한 줄을 그 방, 그 대사, 그 순간과 함께 보관하고, 이벤트 스트림으로 알려
  청취자가 재생하게 합니다. 언제 말할지, 무엇을 말할지는 호출하는 쪽이 정합니다.
- **하네스 플러그인:** `bmo-voice mcp` MCP 서버와 `bmo-voice` 스킬입니다.
- **호스팅 도구:** 도구 호스트의 `voice/servers`입니다. 엔진 포트에 직접 접근하지 않는 세션을 위해, 어떤 음성
  노드가 등록되어 있고 각 서버가 응답하는지 알려 줍니다.

다른 모든 유닛과는 표준 API로만 통신하며(`docs/boundary-api.md`), 다른 저장소 가운데 의존할 수 있는 것은 기반
라이브러리인 `bmo-chat`, `bmo-chat-client`, `bmo-ui`, `bmo-text`뿐입니다.

## 모델

저장소에는 가중치가 없습니다. 음성 노드에는 다음이 필요합니다.

| 무엇 | 받는 곳 | 라이선스 |
|---|---|---|
| Fish Audio S2-Pro 베이스 가중치 | `synth/engine.toml`이 고정한 리비전의 [`fishaudio/s2-pro`](https://huggingface.co/fishaudio/s2-pro). `setup`이 각 노드로 옮깁니다 | Fish Audio Research License: 연구·비상업 용도만 |
| 엔진 코드 fish-speech | `synth/engine.toml`이 고정한 커밋의 [`fishaudio/fish-speech`](https://github.com/fishaudio/fish-speech) | 자체 라이선스 |
| 받아쓰기와 목소리 측정용 Whisper | [`openai/whisper-large-v3-turbo`](https://huggingface.co/openai/whisper-large-v3-turbo) | MIT |
| 학습된 목소리 | 이 저장소의 스크립트로 내 에이전트의 녹음에 직접 학습하거나, BMO 목소리 [`ahas-io/bmo-voice`](https://huggingface.co/ahas-io/bmo-voice)를 voices 폴더 아래에 복사 | 베이스와 같음: Fish Audio Research License |

**Built with Fish Audio.** 이 저장소의 코드는 MIT이지만, S2-Pro 가중치와 그 위에 학습한 목소리는 그렇지 않습니다. 유료 서비스, 회사 내부 업무, 호스팅 API에서 목소리를 쓰려면 먼저 Fish Audio의 라이선스를 받아야 합니다(`docs/voice-plan.md`).

## 폴더

| 폴더 | 내용 |
|---|---|
| `data/` | 코드가 언어별로 읽는 언어 자료(`data/<language>/`). 여기에 있는 한국어 줄은 모두 `korean-ledger.json`에 기록되어 있습니다 |
| `deploy/` | 테스트베드의 배포가 관리하는 유닛(음성 서비스와 스크립트 스튜디오) |
| `docs/` | 코드보다 먼저 오는 문서들. `docs/layout.md`부터 읽으십시오 |
| `plugin/` | 하네스 플러그인의 스킬, `plugin/skills/bmo-voice/` |
| `policy/` | 엔진 자체의 정책 값, `policy/voice.toml` |
| `prompts/` | 엔진이 직접 돌리는 모델 실행에 주는 지시문. 학습 대사 작성기용이며 영어로 되어 있습니다 |
| `scripts/` | 노드 및 학습 스크립트. 독립된 Bun 프로젝트로, 음성 노드 설치·푸시·동기화·상태 확인·발화·카드 어댑터, 음성 설치와 검사, 음성 이름, 카드에 배치되는 학습 단계, 학습 대사 작성을 담고 있습니다 |
| `src/` | 크레이트. 설정, 저장소, 이벤트 로그, 클라이언트, MCP 서버, OpenAPI 문서, 스크립트 규칙, 음성 노드, 라우트, 문서 낭독, 스크립트 스튜디오 서버, 그리고 `voice-server`, `bmo-voice`, `script-studio` 바이너리 |
| `studio/` | 스크립트 스튜디오의 미디어 도구. Python 스크립트가 함께 있는 Bun 프로젝트(`studio/lib/`, `studio/scripts/`, `studio/fixtures/`)이며, 화면인 `studio/client/`는 bmo-ui 기반 Nuxt 프로젝트입니다 |
| `synth/` | 실제로 말하는 부분. Fish S2-Pro 발화기와 음성 서버, 엔진의 고정 설치본(`engine.toml`), 인코더, 전사기, 명료도 측정, 도구 엔트리와 그 스키마, 음성 환경의 고정된 패키지 목록 |
| `tests/` | 이 저장소의 크레이트와 노드 스크립트에 대한 모든 테스트(언어 무관) |
| `tools/` | 음성의 호스팅 도구. 각각 매니페스트·스키마·테스트를 갖춘 워크스페이스 크레이트이며, `tools/servers/`는 세션이 음성 서버 상태를 읽는 `voice/servers` 엔트리입니다 |
| `weights/` | 음성 가중치에 대한 선언. 음성 서버의 카드 선언과 음성 및 기반 모델의 위치를 담으며, 가중치 자체는 넣지 않습니다 |

## 테스트

테스트는 코드 옆이 아니라 `tests/`에 둡니다. 호스팅 도구의 테스트는 해당 폴더의 `tests/`에, 스튜디오 화면의
테스트는 `studio/client/tests/`에 두며, 후자는 그 프로젝트의 자체 검사가 타입 검사 다음에 실행합니다. 노드
스크립트의 테스트 `tests/*.test.ts`는 저장소 루트에서 `bun test`로 실행하고, 스크립트와 함께 타입 검사합니다.
테스트베드의 `personal-data` 검사가 개인정보 규칙(`docs/PERSONAL-DATA.md`)을 지킵니다. 추적되는 파일에는 메일
주소, 홈 경로, 전화번호, 토큰, 사설 네트워크 주소가 들어가지 않습니다. 테스트가 지켜야 할 동작은
`docs/inventory.md`에 기록되어 있습니다.

Python 테스트(`tests/*.py`)는 전용 인터프리터 `tests/.venv`에서 실행합니다. 선언의 setup이
`tests/python-requirements.txt`를 바탕으로 `uv`로 이 환경을 만들며, 테스트가 읽는 패키지는 음성 환경과 같은
버전으로 맞춥니다. 그래서 스위트가 어느 머신에 배치되든 실행됩니다. 다른 인터프리터를 쓰려면
`BMO_VOICE_PYTHON`으로 지정합니다.

## 개인정보

저장소의 어떤 파일에도 개인정보는 없습니다. 값은 설치 환경의 설정에 두고, 예시에는 중립적인 이름을 쓰며,
`docs/PERSONAL-DATA.md`가 정한 규칙은 테스트베드의 `personal-data` 검사가 지킵니다.

## 변경 검사

- `cargo check --workspace --all-targets`: 크레이트와 호스팅 도구의 정적 검사.
- `bmo-testbed check personal-data`: 개인정보 검사.
- `scripts/`에서 `bunx tsc --noEmit`: 스크립트의 정적 검사.
- `studio/client/`에서 `bun run check`: 스튜디오 화면의 정적 검사와 그 테스트.
- `studio/`에서 `bunx tsc --noEmit`, 그리고 `python3 -m py_compile studio/scripts/*.py`: 스튜디오 도구의 정적
  검사.
- `cargo fmt --all --check`와 `cargo clippy --workspace --all-targets -- -D warnings`: 리뷰 단계.

## 버전

버전은 세트의 버전이며, 빌드할 때 git 태그에서 읽습니다. 루트 매니페스트 workspace의 버전은 움직이지 않는
자리표시자입니다(`docs/boundary-api.md`, 「Versions」).
