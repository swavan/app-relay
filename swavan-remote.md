I like you to build tauri (client) application with rust backend and svelte that work like remote desktop (not exactly tho).
Server code also must be in rust however I am thinking we might can use rsocket this application (Give me suggestion whhat would make sense here).


i am someone who have mobile access all time and however accessing computer is crusal for my daily work as well.
Because of the mobile devide portability, it's much easier to work on phone. So what I want a server that let me
access my computer applications directly in my phone. Remember it's not as same as accessing entire desktop.

- Features
  - Server
    - Server should be accesible via ssh and itself is running as deamon process
    - Server shold tell me what are the application avaiable in my computer
    - Server should be able to adjust the application windows size based on the client screen size request (resolution fixes)
    - Server should support Audio, Video, Keyboard, mouse, mic input from client application
    - Server should be able to send me only selected application windows UI to client
    - 
  - Client
    - Should be able to establish ssh tunnel connection with server
    - Click should call server to get the list of avaible app with icons
    - All apps should be display as list or tile view.
    - Should have option to check health of the server
    - Should maintain stable connection with server to check if everything is working or not
    - Application should be responsive for desktop or mobile use-case
    - It should be tauri plugin and css theme should be overridable
    - 

Coding Rules"
- Frontend code should be minimal
- Rust tauri command also should be minimal use service layer as much as possible
- Every single code should have been test (Use docker for all of this in CICD)
  - Unit test
  - Integration test
  - E2E test
- Ever single client or server code must support (mac/windows/linux/ios/android) if any unsupported feature should raise expection not supported currently
- Do not write dead code
- DO not add half bake feature (Feature need to be complete)
- Always review code before push for PR
- ALways run code formatter and linter  before push for PR
- Alaways do TDD for every single feature
- Make sure every single code implementation are well documentated and organize
- Use respected agent to complete work rather main agent do, main agent job is to coordinate only
- Do not make assumption, ask me for approval of the planning
- Always plan before write code
- use mono repo for client and server code
