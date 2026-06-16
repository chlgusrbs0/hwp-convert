# 문서 지도 (Documentation Map)

이 디렉터리는 `hwp-convert`의 방향, 현재 상태, fixture 계획, 아키텍처 경계를 기록한다.

## 어디부터 읽나

작업을 시작하는 모든 사람/에이전트는 먼저 저장소 루트의 **`AGENTS.md`**를 읽는다. 공용 작업 기준과 "현재 프로젝트 사실"(crate 버전, rHWP pin, IR_VERSION, 테스트 수, fixture 목록)의 단일 출처다.

그다음 작업 범위에 맞춰:

| 문서 | 내용 | 언제 보나 |
| --- | --- | --- |
| `ROADMAP.md` | 장기 방향, 마일스톤, 완료 기준, rHWP revision 정책, 전략적 결정 사항 | 무엇을/왜 할지 정할 때 |
| `ARCHITECTURE.md` | rHWP, bridge, Document IR, exporter, RenderSnapshot 레이어 경계 + IR 로드맵 마일스톤(v0-v7.4) | 어디를 고칠지 정할 때 |
| `STATUS.md` | 현재 bridge/exporter 지원 행렬, HWPX fixture 현황, 지속가능성 리스크 | "지원한다"고 말하기 전, 코드 바꾼 후 |
| `FIXTURES.md` | fixture 계획, 관리 규칙, 검증 방법, bridge stats | fixture를 추가/변경할 때 |

fixture별 상세는 `tests/fixtures/<fixture_name>/notes.md`에 있다.

## 핵심 원칙

- rHWP가 지원하는 것과 `hwp-convert`가 변환 결과로 지원하는 것은 다르다.
- 지원 완료는 bridge, IR, exporter, fixture가 함께 있을 때만 말한다.
- 기본 SVG는 semantic/plain-text exporter이며 visual fidelity 경로가 아니다.
- 실제 fixture 없이 정확도를 주장하지 않는다.

## 문서 갱신 규칙

코드가 바뀌면 관련 문서도 함께 바꾼다. 갱신 위치는 `AGENTS.md`의 "문서 갱신 규칙"을 따른다. "현재 사실"(테스트 수 등)은 `AGENTS.md` 한 곳에만 두고, 다른 문서는 그것을 인용/링크한다. 저장소 HEAD 커밋 해시는 자주 바뀌므로 문서에 박지 않는다.
