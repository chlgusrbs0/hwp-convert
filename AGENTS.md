# hwp-convert Agent Guide

이 문서는 Codex, Claude, 또는 다른 자동화 작업자가 이 저장소에서 작업할 때 따라야 하는 기준이다.

프로젝트의 목적은 HWP/HWPX 파일을 정확하게 변환하는 것이다. 새 기능을 빠르게 붙이는 것보다 실제 문서에서 데이터가 사라지지 않게 만드는 것이 중요하다.

## 먼저 읽을 문서

작업을 시작하면 다음 순서로 읽는다.

1. `docs/RHWP_CONVERSION_ROADMAP.md`
2. `docs/ARCHITECTURE.md`
3. `docs/COMPATIBILITY.md`
4. `docs/FIXTURES.md`
5. `tests/fixtures/README.md`
6. `tests/fixture_smoke.rs`
7. 관련 코드: `src/bridge/rhwp.rs`, `src/ir/mod.rs`, `src/exporter.rs`, `src/render/mod.rs`

전체를 다 외우려고 하지 말고, 현재 작업 범위와 관련된 부분을 우선 확인한다.

## 현재 프로젝트 사실

마지막 확인 기준: 2026-06-06 KST

- 이 프로젝트는 Rust CLI다.
- 현재 crate 이름은 `hwp-convert`이고 library target은 `hwp_convert`다.
- 입력 형식은 `.hwp`, `.hwpx`다.
- 현재 CLI 출력 형식은 `txt`, `json`, `markdown`, `html`, `svg`다.
- PDF는 아직 구현되어 있지 않다.
- rHWP dependency는 `Cargo.toml`에 git revision으로 고정되어 있다.
- exporter는 rHWP 타입을 직접 읽지 않는다.
- 기본 `--to svg`는 semantic/plain-text 기반 SVG exporter다.
- `src/render`는 experimental renderer-first diagnostics path다.
- 공식 fixture는 `tests/fixtures` 아래에만 둔다.
- 루트의 `sample.hwp`, `sample.hwpx`, `sample.*` 출력물은 로컬 확인용이고 커밋하지 않는다.

## 최우선 목표

우선순위:

1. 변환 정확도
2. 실제 fixture 기반 검증
3. 조용한 데이터 손실 방지
4. rHWP boundary 유지
5. 출력 형식 확장

많은 포맷을 얕게 지원하려 하지 말고, 현재 포맷에서 문서 구조와 텍스트를 더 정확히 보존한다.

## 금지되는 착각

다음 착각을 피한다.

- rHWP가 지원한다고 해서 hwp-convert도 지원한다고 말하지 않는다.
- parse가 성공한다고 해서 변환이 정확하다고 말하지 않는다.
- JSON에 들어갔다고 해서 HTML/Markdown/TXT/SVG도 지원한다고 말하지 않는다.
- semantic SVG를 visual fidelity 결과로 설명하지 않는다.
- fixture 없이 "지원 완료"라고 쓰지 않는다.
- 문서에 없는 사실을 기억으로 단정하지 않는다.

지원 완료라고 말하려면 다음이 필요하다.

1. rHWP parse 또는 renderer query에서 해당 정보가 나온다.
2. bridge 또는 render adapter가 그 정보를 받는다.
3. `Document IR` 또는 `RenderSnapshot`에 정보가 남는다.
4. exporter가 해당 형식에 맞게 출력한다.
5. fixture 또는 unit test가 회귀를 잡는다.
6. `docs/COMPATIBILITY.md`가 현재 상태를 반영한다.

## 작업 원칙

작업할 때는 다음 원칙을 지킨다.

- 작은 변경으로 시작한다.
- 한 커밋에는 하나의 의도를 담는다.
- 기존 architecture boundary를 먼저 따른다.
- exporter에 rHWP 타입을 직접 들여오지 않는다.
- `Document IR` 확장은 신중하게 한다.
- bridge mapping 개선은 보통 `IR_VERSION` bump 없이 가능하다.
- JSON shape, enum variant, 필수 필드 변경처럼 serialized format이 바뀌면 `IR_VERSION` bump를 검토한다.
- fallback을 추가할 때는 왜 fallback인지 warning 또는 docs에 남긴다.
- 새 fixture를 추가하면 `notes.md`도 함께 쓴다.
- 테스트가 없는 기능 지원 주장은 하지 않는다.

## 정확도 작업 절차

새 문서 요소를 개선할 때는 이 순서로 진행한다.

1. 현재 `docs/COMPATIBILITY.md`에서 해당 요소의 상태를 확인한다.
2. rHWP가 현재 pin에서 어떤 model/API로 그 요소를 노출하는지 확인한다.
3. `src/bridge/rhwp.rs` 또는 `src/render/mod.rs` 중 어느 경로가 맞는지 결정한다.
4. 필요한 경우 fixture를 먼저 만든다.
5. bridge 또는 render adapter를 수정한다.
6. `Document IR`에 이미 표현할 수 있는지 본다.
7. 부족하면 IR 확장과 backward compatibility를 검토한다.
8. exporter를 수정한다.
9. `cargo test`를 실행한다.
10. 관련 docs를 갱신한다.

## fixture 작업 절차

fixture는 변환 정확도의 기준이다.

새 fixture는 다음 구조를 따른다.

```text
tests/fixtures/<fixture_name>/
  input.hwp
  input.hwpx
  notes.md
  expected/
  diagnostics/
```

필수:

- `notes.md`에 문서 내용과 기대 동작을 적는다.
- 가능하면 HWP/HWPX를 같은 의미 내용으로 쌍으로 둔다.
- `tests/fixture_smoke.rs`에 feature-level assertion을 추가한다.
- 처음부터 거대한 golden 비교를 하지 않는다.
- binary resource는 bytes 전체 비교보다 id, extension, media type, non-empty bytes, output file existence를 우선한다.

fixture 우선순위:

1. `basic_text`
2. `table`
3. `merged_table`
4. `style`
5. `image`
6. `link_list`
7. `note_header_footer`
8. `equation_shape_chart`
9. `kitchen_sink`

## rHWP dependency 정책

현재는 rHWP를 git revision에 고정한다.

이유:

- rHWP upstream은 계속 변할 수 있다.
- 변환기 정확도는 fixture로 고정되어야 한다.
- fixture 없이 upstream을 따라가면 regression인지 improvement인지 판단하기 어렵다.

rHWP revision을 올릴 때는 다음을 지킨다.

1. 현재 revision과 새 revision을 기록한다.
2. 관련 upstream 변경을 확인한다.
3. dependency update만 담은 commit과 behavior fix commit을 가능하면 분리한다.
4. `cargo test`를 실행한다.
5. fixture 결과 변화가 개선인지 regression인지 판단한다.
6. `docs/COMPATIBILITY.md`와 `docs/RHWP_CONVERSION_ROADMAP.md` 기준 시점을 갱신한다.

권장 commit message:

- `chore(deps): rhwp revision 갱신`
- `fix(bridge): rhwp 변경에 맞춰 이미지 매핑 보정`
- `test(fixtures): rhwp 갱신 회귀 기대값 정리`

## silent drop 대응

가장 나쁜 실패는 성공처럼 보이는 데이터 손실이다.

따라서 다음 중 하나를 남긴다.

- 명시적인 IR node
- `UnknownBlock`
- `UnknownInline`
- `ConversionWarning`
- exporter fallback text
- fixture notes의 known limitation

아무 흔적 없이 버리는 코드는 새로 만들지 않는다. 기존 코드에서 발견하면 가능한 한 fixture와 함께 고친다.

## exporter 정책

출력 형식마다 보존 목표가 다르다.

- JSON: 최대 구조 보존.
- HTML: 문서 구조, 기본 스타일, 링크, 표, 이미지 asset, note 보존.
- Markdown: Markdown이 표현 가능한 구조 우선. 복잡한 표와 layout은 fallback 허용.
- TXT: 읽기 순서와 텍스트 보존 우선.
- semantic SVG: 현재 평문 시각화. 원본 layout fidelity를 주장하지 않는다.
- visual output: `src/render` 또는 별도 renderer-assisted path에서 다룬다.

exporter 변경 시 확인할 것:

- HTML escaping
- Markdown escaping
- table fallback 조건
- image asset path
- multi-file output collision
- note/link/list fallback
- JSON compatibility

## 문서 갱신 규칙

작업 종류별로 갱신할 문서:

- 장기 방향: `docs/RHWP_CONVERSION_ROADMAP.md`
- 아키텍처 경계: `docs/ARCHITECTURE.md`
- 현재 지원 행렬: `docs/COMPATIBILITY.md`
- fixture 계획: `docs/FIXTURES.md`
- fixture 상세: `tests/fixtures/<fixture_name>/notes.md`
- 에이전트 작업 규칙: `AGENTS.md`

문서에는 확실한 사실과 추정을 분리한다.

좋은 문장:

- "현재 bridge는 이 값을 매핑하지 않는다."
- "현재 fixture가 없으므로 실제 문서 지원 여부는 미확인이다."
- "rHWP upstream README는 해당 요소를 언급하지만, hwp-convert bridge 지원은 별도 확인이 필요하다."

나쁜 문장:

- "완벽 지원"
- "원본과 동일"
- "아마 된다"
- "rHWP가 하니까 우리도 된다"

## 검증 명령

기본 검증:

```bash
cargo test
```

특정 fixture test:

```bash
cargo test --test fixture_smoke
```

문서만 바꿔도 가능하면 `cargo test`를 실행해 현재 기준이 깨지지 않았는지 확인한다.

## Git/commit 규칙

사용자가 커밋 또는 푸시를 요청했을 때만 수행한다.

선호 commit message 형식:

```text
type(scope): 한국어 설명
```

예:

- `test(fixtures): basic_text 입력 fixture 추가`
- `fix(bridge): 표 병합 셀 매핑 보정`
- `feat(exporter): HTML note 출력 개선`
- `docs(roadmap): rHWP 변환 로드맵 추가`
- `chore(deps): rhwp revision 갱신`

작업 단위:

- fixture 추가와 bridge fix를 가능하면 분리한다.
- dependency update와 behavior change를 가능하면 분리한다.
- docs update는 코드 변경과 밀접하면 같은 commit에 둘 수 있지만, 큰 roadmap 문서는 별도 commit이 좋다.

## 다음 작업자가 바로 할 일

현재 가장 중요한 다음 일:

1. `tests/fixtures/basic_text/input.hwp`와 `input.hwpx`를 만든다.
2. `cargo test --test fixture_smoke`를 실행한다.
3. 실패하면 rHWP parse 문제인지 bridge assertion 문제인지 분리한다.
4. 통과하면 `table` fixture로 넘어간다.
5. P0 fixture가 쌓이면 rHWP revision update rehearsal을 한다.

새 출력 형식 추가보다 이 순서를 우선한다.

