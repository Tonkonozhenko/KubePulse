const shot = document.querySelector(".shot");
const zoom = shot.querySelector(".zoom");

shot.addEventListener("mousemove", (event) => {
  const rect = shot.getBoundingClientRect();
  const x = ((event.clientX - rect.left) / rect.width) * 100;
  const y = ((event.clientY - rect.top) / rect.height) * 100;
  zoom.style.transformOrigin = `${x}% ${y}%`;
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
