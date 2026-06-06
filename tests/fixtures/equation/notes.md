# equation fixture

## 목적

HWP 문서의 수식 컨트롤이 Document IR의 `Block::Equation`으로 보존되는지 확인한다.

## 현재 입력

- `input.hwp`: 준비됨
- `input.hwpx`: 아직 없음

HWP fixture는 rHWP 모델을 사용해 생성한 합성 문서다. 실제 HWP 파일 포맷 바이트로 직렬화되어 `bridge::rhwp::read_document` 경로를 통과한다.

## 포함된 기능

- equation control 1개
- equation script: `x over y`
- equation common description: `sample equation`

## 검증 기준

`tests/fixture_smoke.rs`의 `assert_equation_fixture`가 다음을 확인한다.

- `Block::Equation`이 정확히 1개 생성된다.
- equation kind가 `PlainText`로 보존된다.
- equation content가 `x over y`로 보존된다.
- equation fallback text가 `x over y`로 보존된다.

## 주의

현재 bridge는 rHWP의 equation script를 plain-text equation content로 보존한다. 이 fixture는 수식의 시각 렌더링이나 LaTeX/MathML 변환을 검증하지 않는다. 변환 정확도 관점에서는 우선 수식 컨트롤이 손실되거나 unknown block으로 떨어지지 않는지를 확인한다.
