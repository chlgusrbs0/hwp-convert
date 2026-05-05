# HWP/HWPX Bridge Compatibility

이 문서는 현재 `hwp-convert` 코드 기준의 bridge/exporter 지원 상태를 정리한다.
새 Document IR 버전을 제안하는 문서가 아니며, 현재 `IR_VERSION`은 그대로 `6`이다.

판정 기준:

- `예`: 현재 코드 경로에 구현이 있고 기본 동작이 확인된다.
- `부분`: 구현은 있지만 구조 손실, 위치 손실, 형식별 fallback, 누락된 메타데이터가 있다.
- `아니오`: 현재 코드 경로에 구현이 없다.
- `해당 없음`: 현재 저장소에는 개념 자체가 없다.

근거 코드:

- `src/bridge/rhwp.rs`
- `src/exporter.rs`
- `src/util/plain_text.rs`
- 로컬 dependency `rhwp` `src/model/control.rs`

## 현재 지원 표

| 요소 | rhwp parse | bridge mapping | Document IR | exporter 지원 | RenderSnapshot 지원 | 현재 한계 | 다음 작업 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| text | 예 | 예 | 예 | 예<br>TXT/JSON/HTML/Markdown/SVG 모두 처리 | 아니오 | 좌표/페이지 단위 정보가 없고, unsupported control 내부 텍스트는 보존되지 않을 수 있다. | `basic_text` fixture로 줄바꿈, 탭, 빈 줄, 한글/영문 혼합 텍스트를 고정한다. |
| paragraph | 예 | 부분 | 예 | 부분<br>모든 exporter가 문단은 내보내지만 heading/title/caption 구분은 없다. | 아니오 | bridge가 빈 문단을 버리고, `ParagraphRole`을 항상 `Body`로 둔다. | `basic_text` fixture에 빈 문단과 문단 경계 케이스를 넣고 현재 동작을 명시적으로 고정한다. |
| style | 부분 | 부분 | 부분 | 부분<br>JSON은 IR을 그대로 보존, HTML은 bold/italic/underline/strike, font-size, color, background-color, text-align 등 일부 CSS 반영, Markdown은 bold/italic/strike/link만 반영, TXT/SVG는 스타일 소실 | 아니오 | char/para style 일부만 매핑한다. 굵기/기울임/밑줄/취소선, 글꼴명/크기, 전경/배경색, 정렬/간격/들여쓰기, 표/셀 배경색만 다룬다. table style ref, border, padding, percent line spacing, paragraph role 추론은 없다. | `style` fixture로 text style, paragraph style, style ref, table/cell 배경색을 고정한다. |
| table | 예 | 예 | 예 | 부분<br>JSON/HTML은 구조 유지, TXT/SVG는 평문 fallback, Markdown은 단순 표만 표 형태 유지 | 아니오 | 표 폭, 경계선, 캡션, 셀 padding, 열 폭, 정렬 등은 IR로 안 간다. | `table` fixture로 단순 사각형 표와 exporter별 현재 출력을 고정한다. |
| merged table cell | 예 | 예 | 예 | 부분<br>JSON/HTML은 `row_span`/`col_span` 유지, Markdown은 fallback, TXT/SVG는 평문화 | 아니오 | 병합 셀의 시각 배치와 너비 계산은 없다. Markdown에는 병합 셀 표현이 없다. | `merged_table` fixture로 row span/col span과 HTML `rowspan`/`colspan`을 고정한다. |
| image | 예 | 부분 | 예 | 부분<br>JSON은 resource bytes 포함, HTML/Markdown은 출력 파일 stem 기준 `<stem>_assets/images/`에 asset을 저장하고 `<stem>_assets/images/...`로 참조, TXT/SVG는 대체 텍스트 | 아니오 | bridge는 `Picture`를 `Image`와 `ImageResource`로 옮기지만, 배치, wrap, crop, anchor 정보는 없다. bin data가 없으면 `UnknownBlock`으로 떨어진다. `Resource::Binary`는 HTML/Markdown asset으로 쓰지 않는다. | `image` fixture로 resource id, extension/media type, alt/caption, width/height 힌트와 HTML/Markdown 문서별 asset 출력을 고정한다. |
| resource | 부분 | 부분 | 부분 | 부분<br>JSON은 resource store를 보존, HTML/Markdown은 `Resource::Image` bytes를 `<stem>_assets/images/` 디렉터리에 저장, TXT/SVG는 사용 안 함 | 아니오 | 현재 bridge는 image bin data만 `ImageResource`로 넣고 `BinaryResource`는 쓰지 않는다. HTML/Markdown 외 exporter asset bundle 정책은 없다. | `image` fixture에 resource 검증과 `<stem>_assets/images/<resource_file_name>` asset 존재 확인을 같이 넣고, 이후 별도 resource bundle 테스트로 확장한다. |
| header/footer | 예 | 예 | 예 | 부분<br>JSON/HTML/Markdown/TXT/SVG 모두 선형화해서 출력 | 아니오 | 현재 exporter는 페이지 반복 레이아웃이 아니라 본문 앞뒤의 block 묶음으로만 다룬다. `FirstPage` 배치는 bridge에서 생성되지 않는다. | `note_header_footer` fixture로 odd/even placement와 block 내용을 고정한다. |
| footnote/endnote | 예 | 부분 | 예 | 부분<br>JSON/HTML/Markdown/TXT/SVG 모두 note ref와 note body를 출력 | 아니오 | `rhwp`가 정확한 inline 위치를 주지 않아 note ref가 문단 끝에 append된다. 페이지 하단 배치, note separator, 위치 보존은 없다. | `note_header_footer` fixture로 note store, trailing ref, warning 발생 여부를 고정한다. |
| link | 부분 | 부분 | 예 | 부분<br>JSON/HTML/Markdown은 URL 보존, TXT/SVG는 링크 라벨 중심 fallback | 아니오 | hyperlink field range는 inline으로 옮기지만 일부 hyperlink control은 문단 끝 append fallback이다. `title`은 채우지 않고, hyperlink 외 field는 link로 다루지 않는다. | `link_list` fixture로 field-range link와 trailing hyperlink control을 각각 고정한다. |
| list | 부분 | 부분 | 예 | 부분<br>JSON/TXT/Markdown은 prefix 보존, HTML은 실제 `<ul>/<ol>` 없이 문단 앞에 literal prefix만 넣음, SVG는 평문 | 아니오 | bullet/number/outline만 `ListInfo`로 옮기고 list container 구조는 없다. nested/restart 케이스도 fixture가 아직 없다. | `link_list` fixture로 bullet, ordered, nested, restart 케이스를 고정한다. |
| equation | 예 | 부분 | 예 | 부분<br>JSON은 구조 보존, HTML/Markdown/TXT/SVG는 `[equation: ...]` fallback 중심. Markdown은 `EquationKind::Latex`일 때만 `$$...$$`를 쓴다. | 아니오 | bridge가 `EquationKind::PlainText`만 생성한다. LaTeX/MathML 판별, numbering, resource 연결, 시각 렌더링이 없다. Markdown 수식 블록은 `Latex`일 때만 강화되는데 현재 bridge는 그 경로를 만들지 않는다. | `equation_shape_chart` fixture로 equation script와 현재 fallback 출력을 고정한다. |
| shape | 예 | 부분 | 부분 | 부분<br>JSON/HTML/Markdown/TXT/SVG 모두 `[shape: ...]` placeholder/fallback text 위주 | 아니오 | line/rect/ellipse/arc/polygon/curve/group/picture를 받아도 IR에는 `kind`, `fallback_text`, `description`만 남긴다. geometry, border/fill, text box, caption, child shape 정보가 소실된다. | `equation_shape_chart` fixture로 대표 shape의 현재 placeholder 동작을 고정한다. |
| chart | 부분 | 아니오 | 예 | 부분<br>exporter는 `Block::Chart`를 `[chart: ...]` fallback으로 출력할 수 있지만 bridge가 실제 문서에서 그 block을 만들지 못함 | 아니오 | 로컬 `rhwp` source에는 chart tag 흔적이 있지만 `Control::Chart` 같은 bridge-visible model은 없다. 현재 `hwp-convert` bridge 경로에서는 chart를 직접 매핑하지 못한다. | `equation_shape_chart` fixture는 우선 smoke/current-behavior 기록용으로 만들고, 실제 `Chart` block assert는 bridge 경로가 생긴 뒤 추가한다. |
| unknown element | 부분 | 부분 | 예 | 부분<br>JSON/HTML/Markdown/TXT/SVG 모두 `fallback_text`를 우선 사용하고, 없으면 `[unknown: kind]` fallback을 출력 | 아니오 | `Control::Unknown`은 `UnknownBlock`으로 감싸지만, `SectionDef`, `ColumnDef`, `Bookmark`, `Ruby`, `Form`, hyperlink 이외 `Field` 같은 known-but-unmapped control은 그냥 버려진다. `UnknownInline`도 현재 bridge에서 거의 쓰지 않는다. | `kitchen_sink` fixture에 unknown/ignored control이 섞인 문서를 넣고, 최소한 현재 누락 지점을 문서화하는 regression test를 설계한다. |
| render snapshot | 예 | 아니오 | 아니오 | 아니오<br>기본 CLI/exporter 경로의 `--to svg`는 RenderSnapshot visual SVG가 아니라 semantic/plain-text 기반 SVG exporter | 부분<br>`src/render/mod.rs`에 `RenderSnapshot`, `RenderSnapshotSummary`, `render_page_svg`, `write_render_snapshot_visual_check`가 있고 `rhwp::DocumentCore`와 rhwp renderer query API를 사용한다. | RenderSnapshot은 semantic Document IR과 분리된 experimental visual path다. 기본 사용자 경로에는 노출되지 않는다. visual SVG, summary, visual-check helper는 있지만 fidelity는 아직 낮고 이미지, 표, 도형 등은 placeholder 중심이다. | 기본 SVG fixture와 분리해서 RenderSnapshot visual SVG/summary/visual-check smoke를 별도 diagnostics fixture로 둔다. |

## 핵심 관찰

1. 현재 bridge의 가장 안정적인 경로는 `text -> paragraph -> simple table/list/link -> JSON/HTML/Markdown/TXT/SVG`다.
2. 이미지와 resource는 IR까지 들어오며 HTML/Markdown exporter는 `Resource::Image` bytes를 출력 파일 stem 기준 `<stem>_assets/images/`에 저장한다. 예: `out/sample.html`과 `out/sample.md`는 `out/sample_assets/images/image-1.png`를 쓰고 문서에서는 `sample_assets/images/image-1.png`로 참조한다. TXT/SVG와 RenderSnapshot visual path의 asset 처리는 별도다.
3. chart는 bridge 기준으로 사실상 미지원이다. RenderSnapshot은 존재하지만 semantic IR/기본 CLI exporter와 분리된 experimental visual path다.
4. unknown element 처리도 절반만 되어 있다. `UnknownControl`은 잡지만, 많은 known-but-unmapped control은 warning 없이 사라질 수 있다.

## 우선순위

- P0: `basic_text`, `style`, `table`, `merged_table`, `image`
- P1: `link_list`, `note_header_footer`
- P2: `equation_shape_chart`, `kitchen_sink`
