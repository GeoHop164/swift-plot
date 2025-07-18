![Swift Plot Logo](src-tauri/icons/Square150x150Logo.png)

# Swift Plot

A simple, and very much unfinished, Tauri app that takes `XLSX` and `CSV` files and plots them. Features a cursor with a tooltip to show the value of the data series at that point.

## The problem it solves

I frequently have to look at large data sets of numerical values, and plotting them is the easiest way, and although I can do the
`Ctrl + <-,Ctrl + ↑, Ctrl + Shift + ->, Ctrl + Shift + ↓, Alt + N, T, Enter, ->, ->, ->, ->, ->, Ctrl + Shift + ↓, Alt, N, D`
Excel keyboard sequence without even thinking now, it's not optimal for each data file, and seeing actual data points is still a pain. What I really needed was a dedicated app just for plotting a data series, and adding a cursor and tooltip to see what the values are at each point.

## Development

This was a challenge I set myself, to solve a problem in a day - and as you can tell by the absolute state of the codebase and the app's performance, that's exactly what I did.

### The Stack (and why)

I went with React+TS because that's what I'm learning at the moment, it's what all my other projects are using, and it just made sense. But handling dozens of Megabytes of Excel sheets isn't a fun process to put onto the frontend, so I was going to just spin up a Flask backend but it felt unecessary. I've been working with Electron for a while, and have become pretty familiar with it, so naturally, I didn't want to use it again, especially not for a performance-sensitive application. Yes I should have written it native, but if I'm honest, my experience is on the web, and this project was purely for procrastination, so no native yet. Anyway, it felt like a good opportunity to check out Tauri, so that's what I did*.

*\* Asked GPT-4, Claude 3.7 Sonnet, and Gemini 2.5 Pro to do*

## Using it

![Screenshot of Swift Plot showing a data plot](_projectAssets/Screenshot.png)

Pretty simple, Upload File with the button in the top left, click which fields you want, and wait for it to plot. Might have to upload the file twice, I'm working on that.

Tested and working on:

- macOS 12
- macOS 13
- Windows 10
- Windows 11

## Known Issues

- You sometimes have to import a data file twice, otherwise data may be missing
- ~~The progress indicator doesn't work~~ Fixed!
- ~~Large data files still cause lag~~ As fixed as I'm going to put effort into

## Current limitations

- Only one data file
- Only one y-axis scale
- Only CSV and Excel files
- No ability to export (you can just screenshot for now)

## Has this project helped you out?

[<img src="https://cdn.buymeacoffee.com/buttons/default-black.png" style="width:300px; border-radius: 25px">](https://coff.ee/geohop164)
