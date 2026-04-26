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

### Resume

이전 manifest를 기반으로 변환을 이어서 진행할 수 있습니다.

```bash
cargo run -- docs --to txt --recursive --output-dir out --manifest manifest.json --resume
```

### 오류가 발생해도 계속 진행

여러 파일을 변환할 때 일부 파일에서 오류가 발생해도 나머지 파일 변환을 계속할 수 있습니다.

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

특히 `txt`는 순수 텍스트 형식이므로 표, 이미지, 글꼴, 색상, 페이지 배치 같은 정보를 그대로 보존하기 어렵습니다.

`markdown`, `html`, `svg` 출력 역시 현재는 원본 한글 문서의 조판을 완전히 재현하기보다, 추출된 텍스트를 각 포맷으로 표현하는 단계입니다.

표, 이미지, 스타일, 페이지 배치 등은 출력 형식에 따라 제한적으로 지원되거나 향후 지원될 예정입니다.

## Document IR 계획

hwp-convert는 단순한 텍스트 추출기가 아니라, 장기적으로 다양한 출력 형식을 안정적으로 지원하는 문서 변환 도구를 목표로 합니다.

이를 위해 내부에 Document IR, 즉 문서를 표현하는 공통 중간 구조를 도입합니다.

현재 구조는 문단 텍스트 중심 변환에 가깝습니다.

```text
HWP/HWPX
→ 문단 텍스트
→ TXT / JSON / Markdown / HTML / SVG
```

Document IR 도입 후에는 다음 구조를 목표로 합니다.

```text
HWP/HWPX
→ rhwp parser / renderer
→ hwp-convert Document IR
→ TXT / JSON / Markdown / HTML / SVG / PDF
```

Document IR은 여러 exporter가 공통으로 사용할 문서 구조입니다.

이를 통해 장기적으로 다음 요소를 표현할 수 있도록 합니다.

- 문단
- 제목
- 목록
- 표
- 셀 병합
- 이미지
- 링크
- 글꼴
- 글자 크기
- 색상
- 문단 스타일
- 머리말/꼬리말
- 각주/미주
- 수식
- 도형
- 알 수 없는 요소
- 변환 경고

## Document IR Roadmap

`Document IR v0~v7`은 배포 버전이 아니라 내부 설계 로드맵입니다.

Document IR은 한 번에 완성하지 않고, 기존 기능을 깨지 않는 작은 단계부터 점진적으로 확장합니다.

### v0: Paragraph 기반 최소 IR

목표는 기존 텍스트 중심 변환을 깨지 않으면서 내부 구조를 Document IR 기반으로 옮기는 것입니다.

핵심 구조:

```text
Document
→ Section
→ Block
→ Paragraph
→ Inline
→ TextRun
```

v0에서 다루는 요소:

- `Document`
- `Metadata`
- `Section`
- `Block`
- `Paragraph`
- `ParagraphRole`
- `Inline`
- `TextRun`
- `TextStyle`
- `UnknownBlock`
- `UnknownInline`
- `ConversionWarning`

v0에서는 아직 표, 이미지, 리소스 저장소, 각주, 머리말/꼬리말, Layout IR을 본격적으로 구현하지 않습니다.

### v1: Exporter의 IR 기반 전환

목표는 모든 exporter가 `Vec<String>` 같은 단순 문단 목록이 아니라 `Document IR`을 입력으로 사용하도록 바꾸는 것입니다.

주요 작업:

- TXT exporter를 IR 기반으로 변경
- JSON exporter를 IR 구조 출력으로 변경
- Markdown exporter를 IR 기반으로 변경
- HTML exporter를 IR 기반으로 변경
- SVG exporter는 우선 기존 동작을 유지하되 IR에서 평문 텍스트를 추출해 사용

이 단계가 끝나면 exporter가 rhwp나 기존 문단 추출 결과에 직접 의존하지 않고, Document IR을 중심으로 동작하게 됩니다.

### v2: Table IR

목표는 표 구조를 문자열이 아니라 문서 구조로 표현하는 것입니다.

추가 예정 요소:

- `Table`
- `TableRow`
- `TableCell`
- `TableStyle`
- `TableCellStyle`

핵심 원칙:

```text
TableCell
→ Vec<Block>
```

표 셀 안에는 단순 텍스트뿐 아니라 문단, 이미지, 수식, 다른 블록이 들어갈 수 있으므로 `TableCell`은 문자열이 아니라 `Vec<Block>`을 가져야 합니다.

이 단계에서는 HTML과 JSON에서 표 구조를 우선적으로 보존하고, Markdown과 TXT에서는 가능한 범위에서 대체 표현을 제공합니다.

### v3: Image와 ResourceStore

목표는 이미지와 바이너리 리소스를 문서 본문에 직접 중복 저장하지 않고, 리소스 저장소를 통해 관리하는 것입니다.

추가 예정 요소:

- `ResourceStore`
- `ResourceId`
- `Resource`
- `ImageResource`
- `Image`

핵심 구조:

```text
Document
→ ResourceStore
→ ImageResource

Block::Image
→ ResourceId 참조
```

이미지 bytes를 `Block`에 직접 넣지 않고 `ResourceStore`에 저장한 뒤, 본문에서는 `resource_id`로 참조합니다.

이 구조를 통해 HTML/Markdown exporter는 이미지 파일을 따로 저장하고 참조할 수 있으며, JSON exporter는 리소스 메타데이터를 보존할 수 있습니다.

### v4: StyleSheet와 스타일 확장

목표는 글꼴, 글자 크기, 색상, 굵게, 기울임, 밑줄, 문단 정렬, 문단 간격 같은 스타일 정보를 표현하는 것입니다.

추가 또는 확장 예정 요소:

- `StyleSheet`
- `TextStyle`
- `ParagraphStyle`
- `TableStyle`
- `TableCellStyle`
- `Color`
- `Spacing`
- `Indent`
- `Alignment`

핵심 원칙:

- 문서 전체에서 공유되는 스타일은 `StyleSheet`에 둡니다.
- 실제 문단이나 텍스트 런에는 적용된 스타일 또는 스타일 참조를 둡니다.
- 출력 포맷이 지원하지 않는 스타일은 조용히 버리지 않고, 가능한 경우 warning 또는 대체 표현으로 처리합니다.

### v5: Note, Header/Footer, Link, List

목표는 일반 문단 외의 문서 구조를 확장하는 것입니다.

추가 예정 요소:

- `Footnote`
- `Endnote`
- `NoteId`
- `HeaderFooter`
- `Link`
- `ListInfo`

핵심 원칙:

- 각주와 미주는 `usize` index보다 안정적인 `NoteId`로 참조합니다.
- 머리말/꼬리말은 `Section`에 연결합니다.
- 목록은 단순 문자 접두사가 아니라 `ListInfo`로 표현합니다.
- 링크는 텍스트와 URL을 함께 보존합니다.

### v6: Equation, Shape, Chart, Unknown Node 확장

목표는 수식, 도형, 차트처럼 복잡한 객체를 IR에서 잃어버리지 않게 하는 것입니다.

추가 또는 확장 예정 요소:

- `Equation`
- `Shape`
- `Chart`
- `UnknownBlock`
- `UnknownInline`
- `ConversionWarning`

핵심 원칙:

- 처음부터 모든 수식과 도형을 완벽히 변환하지 않아도 됩니다.
- 대신 알 수 없는 요소를 조용히 버리지 않고 `UnknownBlock`, `UnknownInline`, `ConversionWarning`으로 남깁니다.
- 가능한 경우 fallback text, 원본 종류, 변환 실패 이유를 함께 기록합니다.

### v7: Layout IR 또는 rhwp Renderer 연동

목표는 SVG/PDF처럼 원본에 가까운 시각적 출력을 위한 렌더링 계층을 검토하는 것입니다.

Semantic IR은 문서의 의미 구조를 표현합니다.

```text
문단
표
이미지
각주
수식
스타일
```

반면 Layout IR은 실제 보이는 위치를 표현합니다.

```text
페이지
좌표
줄바꿈
텍스트 위치
표 셀 위치
이미지 위치
도형 위치
```

장기적으로는 다음 구조를 검토합니다.

```text
Semantic Document IR
→ HTML / Markdown / JSON / TXT

rhwp renderer 또는 Layout IR
→ SVG / PDF
```

SVG와 PDF에서 원본 문서와 가까운 출력을 제공하려면 단순 Semantic IR만으로는 부족할 수 있습니다. 이 단계에서는 rhwp의 렌더링 결과를 활용하거나, 별도의 Layout IR을 도입하는 방향을 검토합니다.

## Versioning

`Document IR v0~v7`은 배포 버전이 아니라 내부 설계 로드맵입니다.

실제 hwp-convert 배포 버전은 Semantic Versioning을 따릅니다.

- Patch version: 버그 수정, 문서 수정, 내부 리팩터링
- Minor version: 기존 CLI와 출력 호환성을 유지하는 기능 추가
- Major version: CLI, Document IR JSON, 출력 형식 등 기존 사용자의 호환성을 깨는 변경

Document IR 로드맵이 안정화된 이후에는 `v0~v7` 단계명을 배포 버전처럼 사용하지 않고, SemVer 기준으로 버전을 관리합니다.

## Document IR JSON Policy

- `IR_VERSION`은 `Document IR Roadmap v0~v7`과 별개의 serialized Document IR format version이다.
- `IR_VERSION`은 enum variant 추가, 필수 필드 추가, JSON shape 변경처럼 호환성에 영향을 주는 변경에서 올린다.
- 현재 JSON 출력은 외부 교환 포맷이라기보다 Document IR 디버깅과 구조 확인에 가깝다.
- additive field는 구버전 JSON 역직렬화를 위해 가능하면 `#[serde(default)]`를 사용한다.
- resource bytes와 asset bundle 정책은 추후 별도 정리 예정이다.
- 단위 정책은 현재 다음 기준을 따른다.
- 글꼴 크기: pt
- page/paper 같은 physical size: mm
- renderer/layout 좌표: px 또는 추후 Layout IR 전용 단위

## rhwp의 역할

hwp-convert는 HWP/HWPX 파일을 해석하는 parser / renderer backend로 [rhwp](https://github.com/edwardkim/rhwp)를 활용합니다.

hwp-convert의 exporter는 rhwp 내부 타입에 직접 의존하지 않고, rhwp 결과를 hwp-convert의 Document IR로 변환한 뒤 사용합니다.

```text
rhwp
→ hwp-convert Document IR
→ exporters
```

이 구조를 통해 rhwp가 업데이트되더라도 exporter 전체가 흔들리지 않도록 합니다.

hwp-convert는 rhwp를 직접 대체하는 프로젝트가 아니라, rhwp 위에서 다양한 출력 형식으로 변환하는 계층을 만드는 프로젝트입니다.

rhwp 프로젝트와 기여자들에게 깊이 감사드립니다.

## AI-assisted Development

이 프로젝트는 AI 도구의 도움을 받아 개발될 수 있습니다.

AI는 코드 초안 작성, 리팩터링 아이디어, 테스트 초안, 문서 작성 보조 등에 활용될 수 있습니다.  
다만 최종 코드와 설계 결정은 사람이 검토하고 책임지는 것을 원칙으로 합니다.

AI가 생성한 코드는 다음 기준을 통과해야 합니다.

- 코드 포맷 확인
- 테스트 통과
- 필요한 경우 정적 분석 도구 확인
- 기존 기능 회귀 여부 확인
- 프로젝트 목표와 Document IR 로드맵에 맞는지 검토

AI는 개발을 돕는 도구이며, hwp-convert는 사람이 검토하고 유지보수하는 오픈소스 프로젝트를 지향합니다.

## 라이선스

MIT License