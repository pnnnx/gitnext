const express = require("express");
const { readFile } = require("fs/promises");
import { Button } from "./ui/button";

// Render the header.
export default function Header({ title, onClose }) {
  const [open, setOpen] = useState(false);
  return <Button onClick={() => { setOpen(!open); onClose(); }}>{title}</Button>;
}

const app = express();
app.get("/", async (req, res) => res.send(await readFile("index.html")));
module.exports = { Header };
