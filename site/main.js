const shot = document.querySelector(".shot");
const zoom = shot.querySelector(".zoom");
let start = null;

const clamp = (value) => Math.min(100, Math.max(0, value));

const focus = (event) => {
  const rect = shot.getBoundingClientRect();
  const x = clamp(((event.clientX - rect.left) / rect.width) * 100);
  const y = clamp(((event.clientY - rect.top) / rect.height) * 100);
  zoom.style.transformOrigin = `${x}% ${y}%`;
};

shot.addEventListener("pointermove", focus);
shot.addEventListener("pointerdown", (event) => {
  start = { x: event.clientX, y: event.clientY };
});
shot.addEventListener("pointerup", (event) => {
  if (event.pointerType === "mouse" || !start) return;
  const moved = Math.hypot(event.clientX - start.x, event.clientY - start.y);
  start = null;
  if (moved > 10) return;
  focus(event);
  shot.classList.toggle("zoomed");
});

const track = document.querySelector(".track");
const slides = [...track.querySelectorAll(".slide")];
const dots = document.querySelector(".dots");

const show = (index) => {
  const next = (index + slides.length) % slides.length;
  track.scrollTo({ left: slides[next].offsetLeft - track.offsetLeft });
};

const current = () => Math.round(track.scrollLeft / track.clientWidth);

const buttons = slides.map((slide, index) => {
  const button = document.createElement("button");
  button.type = "button";
  button.setAttribute("aria-label", slide.querySelector("figcaption").textContent);
  button.addEventListener("click", () => show(index));
  dots.append(button);
  return button;
});

const mark = () => {
  const active = current();
  buttons.forEach((button, index) => button.setAttribute("aria-current", index === active));
};

document.querySelector(".prev").addEventListener("click", () => show(current() - 1));
document.querySelector(".next").addEventListener("click", () => show(current() + 1));
track.addEventListener("scroll", mark, { passive: true });
track.addEventListener("keydown", (event) => {
  if (event.key === "ArrowLeft") show(current() - 1);
  if (event.key === "ArrowRight") show(current() + 1);
});
mark();
