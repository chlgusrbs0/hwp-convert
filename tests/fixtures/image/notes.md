# image fixture

## 목적

HWP 문서 안의 그림 컨트롤이 Document IR의 `Block::Image`와 `ImageResource`로 함께 보존되는지 확인한다.

이 fixture는 이미지가 단순한 `[image]` 텍스트 fallback이나 `Unknown` 블록으로 떨어지지 않고, 실제 바이너리 리소스와 표시 힌트를 가진 이미지 블록으로 변환되는지를 검증한다.

## 현재 입력

- `input.hwp`: 준비됨
- `input.hwpx`: 아직 없음

HWP fixture는 rHWP 모델을 사용해 생성한 합성 문서다. 실제 HWP 파일 포맷 바이트로 직렬화되어 `bridge::rhwp::read_document` 경로를 통과한다.

## 포함된 기능

- 그림 1개
- embedded PNG binary resource 1개
- resource id: `image-1`
- extension: `png`
- media type: `image/png`
- description/alt text: `sample image`
- display width: `96px`
- display height: `48px`

## 검증 기준

`tests/fixture_smoke.rs`의 `assert_image_fixture`가 다음을 확인한다.

- `Block::Image`가 정확히 1개 생성된다.
- 이미지 블록이 `image-1` 리소스를 참조한다.
- `sample image` 설명이 alt text로 보존된다.
- 7200 x 3600 HWP units가 96 x 48 px 표시 힌트로 보존된다.
- `ImageResource`가 `png` 확장자와 `image/png` media type을 가진다.
- resource bytes가 PNG signature로 시작한다.

## 주의

이 fixture는 이미지 픽셀의 시각적 렌더링 품질을 검증하지 않는다. 현재 프로젝트는 viewer renderer가 아니라 파일 변환기이므로, 이 단계에서는 원본 이미지 리소스와 문서 내 참조가 손실 없이 전달되는지를 우선 검증한다.

HTML/Markdown exporter의 asset 출력도 `official_fixtures_export_all_current_formats`에서 확인한다. 변환 결과가 `input_assets/images/image-1.png` 파일을 만들고, HTML/Markdown 본문이 해당 경로를 참조해야 한다.
