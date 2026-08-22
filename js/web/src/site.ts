for (const button of document.querySelectorAll<HTMLButtonElement>(
  "[data-copy]",
)) {
  button.addEventListener("click", async () => {
    const command = button.dataset.copy!;
    await navigator.clipboard.writeText(command);
    button.dataset.copied = "";
    button.setAttribute("aria-label", `${command}, copied`);
    window.setTimeout(() => {
      delete button.dataset.copied;
      button.removeAttribute("aria-label");
    }, 1500);
  });
}
