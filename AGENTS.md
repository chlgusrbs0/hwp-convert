# hwp-convert Agent Guide

이 문서는 Claude, Codex, 또는 다른 자동화 작업자와 사람이 이 저장소에서 작업할 때 **가장 먼저 읽는 공용 진입점**이다. 여러 에이전트가 같은 우선순위와 사실을 공유하기 위한 단일 기준 문서다.

프로젝트의 목적은 HWP/HWPX 파일을 정확하게 변환하는 것이다. 새 기능을 빠르게 붙이는 것보다 실제 문서에서 데이터가 사라지지 않게 만드는 것이 중요하다.

## 현재 프로젝트 사실 (single source of truth)

> 최종 검증: 2026-07-11. 이 블록이 "현재 상태"의 기준값이다. 다른 문서는 여기를 인용하거나 링크한다.
> 숫자(테스트 수 등)를 인용하기 전에 가능하면 `cargo test`로 다시 확인한다.
> **저장소 HEAD 커밋 해시는 자주 바뀌므로 문서에 박지 않는다.** 필요하면 `git log --oneline`으로 확인한다.

- 이 프로젝트는 Rust CLI다.
- crate 이름은 `hwp-convert`, library target은 `hwp_convert`, edition은 2024다.
- rHWP dependency 고정: `github.com/edwardkim/rhwp` rev `bea635bd708274a51ae3f557a71b07683d7c2454` (rhwp v0.7.3).
- 현재 `IR_VERSION`: `31`.
- 입력 형식: `.hwp`, `.hwpx`.
- 출력 형식: `txt`, `json`, `markdown`, `html`, `svg`. PDF는 미구현.
- 테스트 상태: `cargo test` 통과 — unit test 295개 + fixture smoke 4개. `cargo clippy --all-targets -- -D warnings` 무경고.
- 공식 fixture(10개), `tests/fixtures/` 아래:
  - HWP/HWPX 쌍: `basic_text`, `list`
  - HWP 단독: `equation`, `footnote`, `header_footer`, `image`, `merged_table`, `shape`, `style`, `table`
- 직접 dependency: `serde 1.0.228`, `serde_json 1.0.145`, `zip 8.5.1`.
- 기본 `--to svg`는 semantic/plain-text 기반 SVG exporter다. visual fidelity 경로가 아니다.
- `src/render`는 experimental renderer-first diagnostics path다.
- 루트의 `sample.hwp`, `sample.hwpx`, `sample.*` 출력물은 로컬 확인용이며 커밋하지 않는다.

## 먼저 읽을 문서

작업 범위에 맞춰 다음을 확인한다. 전부 외우지 말고 필요한 부분만 본다.

1. `AGENTS.md` (이 문서) — 공용 작업 기준과 현재 사실.
2. `docs/ROADMAP.md` — 장기 방향, 마일스톤, 완료 기준, rHWP revision 정책.
3. `docs/ARCHITECTURE.md` — 레이어 경계와 Document IR 로드맵 마일스톤.
4. `docs/STATUS.md` — 현재 bridge/exporter 지원 행렬과 HWPX fixture 현황.
5. `docs/FIXTURES.md` — fixture 계획, 관리 규칙, 검증 방법.
6. 관련 코드: `src/bridge/rhwp.rs`, `src/hwpx.rs`, `src/ir/mod.rs`, `src/exporter.rs`, `src/render/mod.rs`.

문서 지도는 `docs/README.md`에 있다.

## 빌드 / 테스트 / 실행

```bash
cargo build
cargo test
cargo clippy --all-targets
```

특정 fixture test만:

```bash
cargo test --test fixture_smoke
```

변환 실행 예:

```bash
cargo run -- sample.hwpx --to txt
cargo run -- docs --to html --recursive --output-dir out
```

Bridge stats expected 갱신 (입력 fixture가 있을 때만):

```bash
# bash
HWP_CONVERT_UPDATE_FIXTURE_STATS=1 cargo test --test fixture_smoke official_fixtures_match_expected_bridge_stats
```

```powershell
# PowerShell
$env:HWP_CONVERT_UPDATE_FIXTURE_STATS='1'; cargo test --test fixture_smoke official_fixtures_match_expected_bridge_stats
```

이 명령은 fixture 입력 파일별로 `expected/bridge-stats.hwp.json` 또는 `expected/bridge-stats.hwpx.json`을 쓴다. 일반 `cargo test`는 expected 파일을 만들거나 고치지 않는다.

문서만 바꿔도 가능하면 `cargo test`를 돌려 현재 기준이 깨지지 않았는지 확인한다.

## 최우선 목표

우선순위:

1. 변환 정확도
2. 실제 fixture 기반 검증
3. 조용한 데이터 손실(silent drop) 방지
4. rHWP boundary 유지
5. 출력 형식 확장

많은 포맷을 얕게 지원하려 하지 말고, 현재 포맷에서 문서 구조와 텍스트를 더 정확히 보존한다.

## 지원 완료의 정의

다음을 모두 갖췄을 때만 "지원한다"고 말한다.

1. rHWP parse 또는 renderer query에서 해당 정보가 나온다.
2. bridge 또는 render adapter가 그 정보를 받는다.
3. `Document IR` 또는 `RenderSnapshot`에 정보가 남는다.
4. exporter가 해당 형식에 맞게 출력한다.
5. fixture 또는 unit test가 회귀를 잡는다.
6. `docs/STATUS.md`가 현재 상태를 반영한다.

피해야 할 착각:

- rHWP가 지원한다고 해서 hwp-convert도 지원한다고 말하지 않는다.
- parse가 성공한다고 해서 변환이 정확하다고 말하지 않는다.
- JSON에 들어갔다고 해서 HTML/Markdown/TXT/SVG도 지원한다고 말하지 않는다.
- semantic SVG를 visual fidelity 결과로 설명하지 않는다.
- fixture 없이 "지원 완료"라고 쓰지 않는다.
- 문서에 없는 사실을 기억으로 단정하지 않는다.

## 작업 원칙

- 작은 변경으로 시작한다. 한 커밋에는 하나의 의도를 담는다.
- 기존 architecture boundary를 먼저 따른다 (`docs/ARCHITECTURE.md`).
- exporter에 rHWP 타입을 직접 들여오지 않는다. rHWP 타입은 `src/bridge`와 `src/render` 뒤에 둔다.
- `Document IR` 확장은 신중하게 한다. bridge mapping 개선은 보통 `IR_VERSION` bump 없이 가능하다.
- JSON shape, enum variant, 필수 필드 변경처럼 serialized format이 바뀌면 `IR_VERSION` bump를 검토한다.
- fallback을 추가할 때는 왜 fallback인지 warning 또는 docs에 남긴다.
- 새 fixture를 추가하면 `notes.md`도 함께 쓴다.
- 테스트가 없는 기능 지원 주장은 하지 않는다.

## 정확도 작업 절차

새 문서 요소를 개선할 때:

1. `docs/STATUS.md`에서 해당 요소의 현재 상태를 확인한다.
2. rHWP가 현재 pin에서 어떤 model/API로 그 요소를 노출하는지 확인한다.
3. `src/bridge/rhwp.rs`, `src/hwpx.rs`, `src/render/mod.rs` 중 어느 경로가 맞는지 결정한다.
4. 필요하면 fixture를 먼저 만든다.
5. bridge 또는 render adapter를 수정한다.
6. `Document IR`로 이미 표현 가능한지 본다. 부족하면 IR 확장과 backward compatibility를 검토한다.
7. exporter를 수정한다.
8. `cargo test`를 실행한다.
9. 관련 docs를 갱신한다.

## silent drop 대응

가장 나쁜 실패는 성공처럼 보이는 데이터 손실이다. 따라서 다음 중 하나를 남긴다.

- 명시적인 IR node
- `UnknownBlock` / `UnknownInline`
- `ConversionWarning`
- exporter fallback text
- fixture `notes.md`의 known limitation

아무 흔적 없이 버리는 코드는 새로 만들지 않는다. 기존 코드에서 발견하면 가능한 한 fixture와 함께 고친다.

## exporter 정책

- JSON: 최대 구조 보존.
- HTML: 문서 구조, 기본 스타일, 링크, 표, 이미지 asset, note 보존.
- Markdown: Markdown native 표현이 가능한 구조 우선. 복잡한 표와 layout은 fallback 허용.
- TXT: 읽기 순서와 텍스트 보존 우선.
- semantic SVG: 현재 평문 시각화. 원본 layout fidelity를 주장하지 않는다.
- visual output: `src/render` 또는 별도 renderer-assisted path에서 다룬다.

exporter 변경 시 확인: HTML escaping, Markdown escaping, table fallback 조건, image asset path, multi-file output collision, note/link/list fallback, JSON compatibility.

## rHWP dependency 정책

현재는 rHWP를 git revision에 고정한다. 이유: upstream은 계속 변할 수 있고, 변환기 정확도는 fixture로 고정되어야 하며, fixture 없이 upstream을 따라가면 regression인지 improvement인지 판단하기 어렵다.

revision을 올릴 때:

1. 현재 revision과 새 revision을 기록한다.
2. 관련 upstream 변경을 확인한다.
3. dependency update commit과 behavior fix commit을 가능하면 분리한다.
4. `cargo test`를 실행한다.
5. fixture 결과 변화가 개선인지 regression인지 판단한다.
6. `docs/STATUS.md`와 `docs/ROADMAP.md`의 기준 시점을 갱신한다.

## 문서 갱신 규칙

작업 종류별 갱신 위치:

- 장기 방향: `docs/ROADMAP.md`
- 아키텍처 경계: `docs/ARCHITECTURE.md`
- 현재 지원 행렬 / HWPX fixture 현황: `docs/STATUS.md`
- fixture 계획·관리: `docs/FIXTURES.md`
- fixture 상세: `tests/fixtures/<fixture_name>/notes.md`
- 에이전트 작업 규칙 / 현재 사실: `AGENTS.md`
- 사용자-facing 사용법: `README.md`

문서에는 확실한 사실과 추정을 분리한다.

좋은 문장: "현재 bridge는 이 값을 매핑하지 않는다." / "현재 fixture가 없으므로 실제 문서 지원 여부는 미확인이다."
나쁜 문장: "완벽 지원" / "원본과 동일" / "아마 된다" / "rHWP가 하니까 우리도 된다".

## Git / commit 규칙

사용자가 커밋 또는 푸시를 요청했을 때만 수행한다.

commit message 형식: `type(scope): 한국어 설명`

예:
- `test(fixtures): basic_text 입력 fixture 추가`
- `fix(bridge): 표 병합 셀 매핑 보정`
- `feat(exporter): HTML note 출력 개선`
- `docs(roadmap): rHWP 변환 로드맵 갱신`
- `chore(deps): rhwp revision 갱신`

작업 단위: fixture 추가와 bridge fix를 분리, dependency update와 behavior change를 분리. 큰 roadmap 문서 변경은 별도 commit이 좋다.

## 다음 작업자가 바로 할 일

현재 가장 중요한 다음 일은 **실제 문서 fixture 확보**다.

1. `tests/fixtures/basic_text/`처럼, 나머지 P0 fixture의 실제 `input.hwp`/`input.hwpx`를 채운다.
2. `cargo test --test fixture_smoke`를 실행한다.
3. 실패하면 rHWP parse 문제인지 bridge assertion 문제인지 분리한다.
4. P0 fixture가 쌓이면 rHWP revision update rehearsal을 한다.

새 출력 형식 추가보다 이 순서를 우선한다. 자세한 배경은 `docs/ROADMAP.md`.
