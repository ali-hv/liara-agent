I want to create a desktop application(for now for linux=.deb).
this app is for users to easily deploy their project to liara.
this app should make the process autmatic and only ask user critical questions that user must decide and stuff that our agent can't do itself.
for this app you should use tauri with these:
React + TypeScript for the interface.
CSS or Tailwind for styling.

the app should be pretty and modern with great ui/ux so even non tech users which just vibe coded a project can easily use it to deploy their project.
it should also support persian language besides english(when switched to persian, it should become rtl).

i provided the challenge-description.md and video-guide.md which are explanations of what this project should be and do. attention that for the first phase, don't implement the SRE part, because that should be a different agent running 24h on the server for monitoring the deployed services but this app is running on users computers so for now just ignore the sre part.

i also put the liara related links in liara-api.txt which includes liara api docs and liara manual docs. before you do anything, fully scrape those pages and their subpages and list everything that we can do with liara apis in a document cause this app should be connected and integrated with liara apis all the time.

now i want this app to be like this:
first of all we need api token of the user to be able to connect to his account so we should show them a video inside our app to show them how to create and get that token and give it to us. then we should have an input to take that from them and after that we should test that with a request and confirm the token is right and we are connected to user account. the video guide is get-liara-api.mp4

from now on, the whole process is handling with an ai agent(use openai format and read the baseurl, model and api key from env).

then user should add project by selecting their project folder and we should smartly detect everything we can such as does it have git, the project stack, does it have docker, the images and every possible thing(remember we have an ai agent).

then we should guide user through deployment and ask him stuff like suggest him to use that liara plan for this project and tell him the reason but he can choose another liara plan if he wants we won't force him, and implement everything, be smart and creative and create a zero to deployment for any type of user with any type of tech knowledge, read and go through challenge-description and video-guide.md

at the end, don't need to give me the .deb file for now cause we will do changes a lot so don't waste time exporting .deb for now, just bring up the project after you finished and let me test it.

also for persian font, use yekan bakh font which i put in here.