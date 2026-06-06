# Documentation Map

## Current addendum

- `HWPX_FIXTURE_FINDINGS.md`
  - Records which paired HWPX fixtures are accepted at the current rHWP pin and which synthetic HWP to HWPX attempts were rejected because they lost structure.
  - Read this before adding `input.hwpx` to an existing fixture.

이 디렉터리는 `hwp-convert`의 장기 방향, 현재 지원 상태, fixture 계획, 아키텍처 경계를 기록한다.

먼저 읽을 문서:

1. `RHWP_CONVERSION_ROADMAP.md`
   - rHWP 기반 파일 변환기로 가기 위한 장기 로드맵.
   - 시간 추정, 마일스톤, 완료 기준, rHWP revision 정책을 포함한다.

2. `ARCHITECTURE.md`
   - rHWP, bridge, semantic `Document IR`, exporter, RenderSnapshot의 경계를 설명한다.

3. `COMPATIBILITY.md`
   - 현재 코드 기준의 bridge/exporter 지원 행렬.
   - 새 기능을 지원한다고 말하기 전에 반드시 이 문서를 확인하고 갱신한다.

4. `FIXTURES.md`
   - 실제 HWP/HWPX fixture corpus 계획.
   - 변환 정확도 회귀 테스트를 어떻게 만들지 설명한다.

작업자용 지침:

- 루트의 `AGENTS.md`를 읽는다.
- 자동화 작업자와 사람이 같은 우선순위를 유지하기 위한 규칙이 들어 있다.

핵심 원칙:

- rHWP가 지원하는 것과 `hwp-convert`가 변환 결과로 지원하는 것은 다르다.
- 지원 완료는 bridge, IR, exporter, fixture가 함께 있을 때만 말한다.
- 현재 기본 SVG는 semantic/plain-text exporter이며 visual fidelity 경로가 아니다.
- 실제 fixture 없이 정확도를 주장하지 않는다.
