# hwp-convert

hwp-convert는 한컴오피스, Windows COM, 유료 오피스 프로그램 없이 HWP/HWPX 문서를 여러 형식으로 변환하기 위한 Rust 기반 CLI 도구입니다.

한국에서 널리 사용되는 HWP/HWPX 문서를 더 자유롭게 다루기 위한 오픈소스 변환 도구를 목표로 합니다.

## 목표

- 한컴오피스 없이 HWP/HWPX 문서를 변환합니다.
- Windows COM 자동화에 의존하지 않습니다.
- 특정 운영체제나 유료 오피스 프로그램에 묶이지 않는 변환 도구를 지향합니다.
- [rhwp](https://github.com/edwardkim/rhwp)를 HWP/HWPX 파싱 및 렌더링 기반 엔진으로 활용합니다.
- TXT, JSON, Markdown, HTML, SVG, PDF 등 다양한 출력 형식으로 확장 가능한 변환 구조를 목표로 합니다.

## 현재 지원 형식

### 입력

- `.hwpx`
- `.hwp`

HWP와 HWPX는 내부 구조가 다르므로 지원 수준이 다를 수 있습니다.

### 출력

현재 지원 또는 실험 중인 출력 형식은 다음과 같습니다.

- `txt`
- `json`
- `markdown`
- `html`
- `svg`

PDF는 아직 구현되어 있지 않습니다.

## 사용법

기본 사용법은 다음과 같습니다.

```bash
hwp-convert <입력 파일 또는 디렉터리> --to <출력 형식>
```

개발 중에는 Cargo를 통해 실행할 수 있습니다.

```bash
cargo run -- <입력 파일 또는 디렉터리> --to <출력 형식>
```

## 사용 예시

### 단일 파일 변환

```bash
cargo run -- sample.hwpx --to txt
cargo run -- sample.hwpx --to json
cargo run -- sample.hwpx --to markdown
cargo run -- sample.hwpx --to html
cargo run -- sample.hwpx --to svg
```

### 출력 폴더 지정

```bash
cargo run -- sample.hwpx --to txt --output-dir out
```

### 디렉터리 재귀 변환

```bash
cargo run -- docs --to txt --recursive --output-dir out
```

### Manifest 생성

변환 결과를 manifest 파일로 기록할 수 있습니다.

```bash
cargo run -- docs --to txt --recursive --output-dir out --manifest manifest.json
```

성공한 manifest 항목은 복구 가능한 데이터 손실 warning(HWPX preview fallback, 미지원 control 등)이 있을 때 `warning_count`와 `warnings`를 포함합니다. CLI도 총 warning 수와 처음 몇 개의 메시지를 출력합니다. 모든 per-file warning이 필요하면 `--manifest`를 사용하세요.

### Resume

이전 manifest를 기반으로 변환을 이어서 진행할 수 있습니다.

```bash
cargo run -- docs --to txt --recursive --output-dir out --manifest manifest.json --resume
```

### 오류가 발생해도 계속 진행

```bash
cargo run -- docs --to txt --recursive --output-dir out --continue-on-error
```

### 이미 존재하는 출력 파일 건너뛰기

```bash
cargo run -- docs --to txt --recursive --output-dir out --skip-existing
```

## 현재 한계

hwp-convert는 아직 초기 단계입니다.

현재 출력은 원본 문서의 조판을 완벽히 재현하는 단계가 아니라, 추출된 텍스트와 문서 구조를 각 출력 형식에 맞게 표현하는 단계에 가깝습니다.

특히 `txt`는 순수 텍스트 형식이므로 표, 이미지, 글꼴, 색상, 페이지 배치 같은 정보를 그대로 보존하기 어렵습니다. `markdown`, `html`, `svg` 출력 역시 현재는 원본 조판을 완전히 재현하기보다 추출된 내용을 각 포맷으로 표현하는 단계입니다.

표, 이미지, 스타일, 페이지 배치 등은 출력 형식에 따라 제한적으로 지원되거나 향후 지원될 예정입니다. 요소별 현재 지원 수준은 [`docs/STATUS.md`](docs/STATUS.md)에 정리되어 있습니다.

## 설계와 내부 문서

hwp-convert는 단순한 텍스트 추출기가 아니라, 다양한 출력 형식을 안정적으로 지원하기 위해 내부에 공통 중간 구조인 **Document IR**을 둡니다.

```text
HWP/HWPX → rhwp parser / src/hwpx.rs → hwp-convert Document IR → TXT / JSON / Markdown / HTML / SVG (→ 향후 PDF)
```

내부 설계와 작업 기준은 다음 문서에 있습니다.

- [`AGENTS.md`](AGENTS.md) — 작업 기준과 현재 프로젝트 사실(단일 출처)
- [`docs/ROADMAP.md`](docs/ROADMAP.md) — 장기 방향, 마일스톤, Document IR 단계 계획
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) — 레이어 경계와 Document IR 마일스톤(v0-v7.4)
- [`docs/STATUS.md`](docs/STATUS.md) — 현재 요소별 지원 행렬과 HWPX 현황
- [`docs/FIXTURES.md`](docs/FIXTURES.md) — fixture 계획과 검증 방법

## Versioning

실제 hwp-convert 배포 버전은 Semantic Versioning을 따릅니다.

- Patch: 버그 수정, 문서 수정, 내부 리팩터링
- Minor: 기존 CLI와 출력 호환성을 유지하는 기능 추가
- Major: CLI, Document IR JSON, 출력 형식 등 기존 사용자의 호환성을 깨는 변경

`Document IR v0~v7`은 배포 버전이 아니라 내부 설계 로드맵 단계입니다. serialized Document IR format version인 `IR_VERSION`은 이와 별개로, enum variant 추가·필수 필드 추가·JSON shape 변경처럼 호환성에 영향을 주는 변경에서 올립니다.

## rhwp의 역할

hwp-convert는 HWP/HWPX 파일을 해석하는 parser / renderer backend로 [rhwp](https://github.com/edwardkim/rhwp)를 활용합니다.

hwp-convert의 exporter는 rhwp 내부 타입에 직접 의존하지 않고, rhwp 결과를 hwp-convert의 Document IR로 변환한 뒤 사용합니다.

```text
rhwp → hwp-convert Document IR → exporters
```

이 구조를 통해 rhwp가 업데이트되더라도 exporter 전체가 흔들리지 않도록 합니다. hwp-convert는 rhwp를 직접 대체하는 프로젝트가 아니라, rhwp 위에서 다양한 출력 형식으로 변환하는 계층을 만드는 프로젝트입니다. rhwp 프로젝트와 기여자들에게 깊이 감사드립니다.

## AI-assisted Development

이 프로젝트는 AI 도구의 도움을 받아 개발될 수 있습니다. AI는 코드 초안, 리팩터링 아이디어, 테스트 초안, 문서 작성 보조 등에 활용될 수 있으나, 최종 코드와 설계 결정은 사람이 검토하고 책임지는 것을 원칙으로 합니다.

AI가 생성한 코드는 코드 포맷 확인, 테스트 통과, 필요한 경우 정적 분석, 기존 기능 회귀 여부, 프로젝트 목표/로드맵 부합 여부를 통과해야 합니다.

## 라이선스

MIT License
