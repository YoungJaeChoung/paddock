<!-- 다음 릴리스 노트. make release가 이 파일을 release-notes/v<버전>.md로 옮겨 릴리스 본문으로 쓴다.
     영어, Major changes / Minor changes 불렛, 한 줄에 한 변경. 해당 없는 절은 지운다. -->
### Minor changes
- Typed characters appear right away; terminal output batching no longer delays each key on Windows
- In WSL folders, only Git commands that open an editor use Windows Git; status and diff stay on WSL Git
- Conversation lookup checks the running CLI's config folder first and limits full scans, fixing 3–6 s backend stalls
