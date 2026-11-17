// Fabrique la fixture vidéo des e2e : dix secondes de formes qui bougent et un son, en WebM
// (VP9, Opus), enregistrées par le Chromium de Playwright (MediaRecorder). Usage :
//   node make-video.mjs <apps/web> <brut.webm>
// puis, pour une vidéo où l on se déplace (durée et index), sans la recoder, par le ffmpeg de Playwright :
//   ffmpeg -i <brut.webm> -c copy video.webm
import { createRequire } from "node:module";
import { writeFileSync } from "node:fs";
import { join } from "node:path";

const [web, out] = process.argv.slice(2);
const require = createRequire(join(web, "package.json"));
const { chromium } = require("@playwright/test");

const browser = await chromium.launch({ args: ["--autoplay-policy=no-user-gesture-required"] });
const page = await browser.newPage();
await page.goto("about:blank");
const base64 = await page.evaluate(async () => {
  const canvas = document.createElement("canvas");
  canvas.width = 320;
  canvas.height = 180;
  document.body.append(canvas);
  const g = canvas.getContext("2d");
  let frame = 0;
  const draw = () => {
    frame += 1;
    g.fillStyle = "#f6f5f1";
    g.fillRect(0, 0, 320, 180);
    for (let i = 0; i < 400; i++) {
      g.fillStyle = `hsl(${(i * 37 + frame * 5) % 360} 70% 50%)`;
      g.fillRect((i * 53 + frame * 3) % 320, (i * 29 + frame * 2) % 180, 24, 24);
    }
    g.fillStyle = "#2f2e2b";
    g.font = "20px sans-serif";
    g.fillText(`Mementum ${frame}`, 12, 28);
  };
  const timer = setInterval(draw, 40);
  const audio = new AudioContext();
  const tone = audio.createOscillator();
  tone.frequency.value = 440;
  const quiet = audio.createGain();
  quiet.gain.value = 0.05;
  const sink = audio.createMediaStreamDestination();
  tone.connect(quiet).connect(sink);
  tone.start();
  const stream = new MediaStream([...canvas.captureStream(25).getVideoTracks(), ...sink.stream.getAudioTracks()]);
  const recorder = new MediaRecorder(stream, {
    mimeType: "video/webm;codecs=vp9,opus",
    videoBitsPerSecond: 1_200_000,
    audioBitsPerSecond: 32_000,
  });
  const chunks = [];
  recorder.ondataavailable = (event) => chunks.push(event.data);
  const stopped = new Promise((done) => (recorder.onstop = done));
  recorder.start();
  await new Promise((done) => setTimeout(done, 10_000));
  recorder.stop();
  await stopped;
  clearInterval(timer);
  const bytes = new Uint8Array(await new Blob(chunks).arrayBuffer());
  let text = "";
  for (let i = 0; i < bytes.length; i += 0x8000) text += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(text);
});
await browser.close();
writeFileSync(out, Buffer.from(base64, "base64"));
console.log(`${out} : ${Buffer.from(base64, "base64").length} octets`);
