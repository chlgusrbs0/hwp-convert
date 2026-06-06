# shape fixture

## 목적

HWP 문서의 도형 컨트롤이 Document IR의 `Block::Shape`로 보존되는지 확인한다.

## 현재 입력

- `input.hwp`: 준비됨
- `input.hwpx`: 아직 없음

HWP fixture는 rHWP 모델을 사용해 생성한 합성 문서다. 실제 HWP 파일 포맷 바이트로 직렬화되어 `bridge::rhwp::read_document` 경로를 통과한다.

## 포함된 기능

- rectangle shape 1개
- description: `sample rectangle`
- fallback text: `sample rectangle`

## 검증 기준

`tests/fixture_smoke.rs`의 `assert_shape_fixture`가 다음을 확인한다.

- `Block::Shape`가 정확히 1개 생성된다.
- shape kind가 `Rectangle`으로 보존된다.
- shape description이 보존된다.
- shape fallback text가 보존된다.

## 주의

이 fixture는 도형의 시각적 렌더링, 선/채우기 스타일, 좌표를 검증하지 않는다. 현재 프로젝트는 viewer renderer가 아니라 파일 변환기이므로, 이 단계에서는 도형 컨트롤이 손실되거나 unknown block으로 떨어지지 않는지를 우선 검증한다.
