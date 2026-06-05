# footnote fixture

## 목적

HWP 문서의 각주 컨트롤이 Document IR의 본문 `Inline::FootnoteRef`와 note store로 함께 보존되는지 확인한다.

## 현재 입력

- `input.hwp`: 준비됨
- `input.hwpx`: 아직 없음

HWP fixture는 rHWP 모델을 사용해 생성한 합성 문서다. 실제 HWP 파일 포맷 바이트로 직렬화되어 `bridge::rhwp::read_document` 경로를 통과한다.

## 포함된 기능

- body paragraph 1개
- footnote 1개
- footnote number: `3`
- body text: `body text`
- note body text: `note body`

## 검증 기준

`tests/fixture_smoke.rs`의 `assert_footnote_fixture`가 다음을 확인한다.

- note store에 note가 정확히 1개 생성된다.
- note id가 `footnote-3`으로 보존된다.
- note kind가 `Footnote`로 보존된다.
- note body text가 보존된다.
- 본문 문단의 마지막 inline이 `FootnoteRef { note_id: "footnote-3" }`로 보존된다.
- rHWP가 현재 정확한 inline 위치를 노출하지 않는 제한을 warning으로 남긴다.

## 주의

rHWP의 현재 모델은 footnote/endnote 컨트롤의 정확한 문자 위치를 bridge에 제공하지 않는다. 그래서 현재 변환기는 note reference를 문단 끝에 append하고 warning을 남긴다. 이 fixture는 그 한계를 숨기지 않고 명시적으로 고정한다.

향후 rHWP가 정확한 위치 정보를 제공하면, 이 fixture의 warning/위치 기대값은 실제 개선과 함께 갱신해야 한다.
