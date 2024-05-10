import { useState } from "react";
import { invoke } from "@tauri-apps/api/tauri";


export default function App() {
  const [greetMsg, setGreetMsg] = useState("");
  const [name, setName] = useState("");
  const [encryptionEnabled, setEncryptionEnabled] = useState(false);
  const [periodicCaptureEnabled, setPeriodicCaptureEnabled] = useState(false);
  const [clickEventEnabled, setClickEventEnabled] = useState(false);


  async function greet() {
    // Learn more about Tauri commands at https://tauri.app/v1/guides/features/command
    setGreetMsg(await invoke("greet", { name }));
  }

  return (
    <div className="container">
      <h1>Welcome to Tauri!</h1>
      <form
        className="row"
        onSubmit={(e) => {
          e.preventDefault();
          greet();
        }}
      >
        <input
          id="greet-input"
          onChange={(e) => setName(e.currentTarget.value)}
          placeholder="Enter a name..."
        />
        <button type="submit">Greet</button>
      </form>

      <br />

      {/* Button that allow the user to clear the image history */}
      <button
        onClick={async () => {
          await invoke("clear_image_history");
        }}
      >
        Clear Image History
      </button>

      <br />

      {/* Toggle button to enabled/disable encryption */}
      <button
        onClick={async () => {
          setEncryptionEnabled(!encryptionEnabled);
          await invoke("toggle_encryption", { enable: !encryptionEnabled });
        }}
      >
        {encryptionEnabled ? "Disable" : "Enable"} Encryption
      </button>

      <br />

      {/* Toggle button to enabled/disable periodic capture */}
      <button
        onClick={async () => {
          setPeriodicCaptureEnabled(!periodicCaptureEnabled);
          await invoke("toggle_periodic_capture", { enable: !periodicCaptureEnabled });
        }}
      >
        {periodicCaptureEnabled ? "Disable" : "Enable"} Periodic Capture
      </button>

      <br />

      {/* Toggle button to enabled/disable click event */}
      <button
        onClick={async () => {
          setClickEventEnabled(!clickEventEnabled);
          await invoke("toggle_click_event", { enable: !clickEventEnabled });
        }}
      >
        {clickEventEnabled ? "Disable" : "Enable"} Click Event
      </button>
        
    
      <p>{greetMsg}</p>
    </div>
  );
}
