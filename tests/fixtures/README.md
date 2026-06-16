# Bridge Coverage Fixtures

이 디렉터리는 HWP/HWPX bridge coverage를 검증하는 공식 fixture 위치다.

fixture 계획, 관리 규칙, 권장 구조, assertion 우선순위, bridge stats expectation 등 전체 내용은 **`docs/FIXTURES.md`**를 본다. 채택된 fixture와 HWPX 쌍 현황은 `docs/STATUS.md`에 있다.

## 빠른 참고

- 공식 fixture는 `tests/fixtures/<fixture_name>/` 아래에만 둔다.
- 저장소 루트의 `sample.hwp`, `sample.hwpx`, `sample.*`는 로컬 확인용이며 커밋하지 않는다.
- `tests/fixture_smoke.rs`는 `input.hwp`/`input.hwpx`를 자동 발견한다. 입력 파일이 없으면 테스트는 준비 상태로 통과한다.
- HWPX paired fixture는 매칭 HWP fixture와 같은 feature-level assertion을 통과할 때만 추가한다. 통과를 위해 assertion을 약화하지 않는다.

각 fixture의 내용과 한계는 해당 폴더의 `notes.md`에 적는다.
