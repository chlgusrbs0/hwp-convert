# list fixture

## 목적

HWP 문서의 목록 문단이 Document IR의 `Paragraph.list` 정보로 보존되는지 확인한다.

## 현재 입력

- `input.hwp`: 준비됨
- `input.hwpx`: 아직 없음

HWP fixture는 rHWP 모델을 사용해 생성한 합성 문서다. 실제 HWP 파일 포맷 바이트로 직렬화되어 `bridge::rhwp::read_document` 경로를 통과한다.

## 포함된 기능

- unordered/bullet paragraph 1개
- ordered paragraph 2개
- bullet text: `bullet item`
- ordered text:
  - `first item`
  - `second item`
- ordered numbering:
  - `first item`: `1`
  - `second item`: `2`

## 검증 기준

`tests/fixture_smoke.rs`의 `assert_list_fixture`가 다음을 확인한다.

- list paragraph가 정확히 3개 생성된다.
- bullet paragraph가 `ListKind::Unordered`로 보존된다.
- bullet level이 `0`으로 보존된다.
- ordered paragraph들이 `ListKind::Ordered`로 보존된다.
- ordered numbering state가 `1`, `2`로 이어진다.

## 주의

이 fixture는 bullet marker 문자를 정확도 기준으로 삼지 않는다. 현재 synthetic HWP 재파싱에서는 bullet kind는 보존되지만 marker 문자가 안정적인 기준으로 돌아오지 않는다. 실제 문서 fixture에서 marker 보존이 확인되면 별도 fixture나 assertion으로 승격한다.
