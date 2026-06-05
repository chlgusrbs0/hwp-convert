# header_footer fixture

## 목적

HWP 문서의 머리말/꼬리말 컨트롤이 본문 블록으로 섞이지 않고 Document IR의 section metadata에 `headers`/`footers`로 보존되는지 확인한다.

## 현재 입력

- `input.hwp`: 준비됨
- `input.hwpx`: 아직 없음

HWP fixture는 rHWP 모델을 사용해 생성한 합성 문서다. 실제 HWP 파일 포맷 바이트로 직렬화되어 `bridge::rhwp::read_document` 경로를 통과한다.

## 포함된 기능

- header 1개
- footer 1개
- header placement: both/default
- footer placement: even page
- header text: `header text`
- footer text: `footer text`

## 검증 기준

`tests/fixture_smoke.rs`의 `assert_header_footer_fixture`가 다음을 확인한다.

- body block이 비어 있어 header/footer 컨트롤이 본문으로 새지 않는다.
- section header가 정확히 1개 생성된다.
- section footer가 정확히 1개 생성된다.
- header placement가 `Default`로 보존된다.
- footer placement가 `EvenPage`로 보존된다.
- header/footer 안의 문단 텍스트가 보존된다.

## 주의

이 fixture는 페이지별 반복 렌더링 위치를 검증하지 않는다. 현재 프로젝트는 viewer renderer가 아니라 파일 변환기이므로, 이 단계에서는 문서 구조와 텍스트가 IR/export 경로에 남는지를 우선 검증한다.
