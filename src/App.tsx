import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/tauri";

export default function App() {
  const [encryptionEnabled, setEncryptionEnabled] = useState(false);
  const [periodicCaptureEnabled, setPeriodicCaptureEnabled] = useState(false);
  const [clickEventEnabled, setClickEventEnabled] = useState(false);
  const [imageUrl, setImageUrl] = useState('');
  const [errorMessage, setErrorMessage] = useState('');

  useEffect(() => {
    (async () => {
      setEncryptionEnabled(await invoke("get_encryption_status"));
      setPeriodicCaptureEnabled(await invoke("get_periodic_capture_status"));
      setClickEventEnabled(await invoke("get_click_event_status"));
    })();
  }, []);

  // Display error message for 10 seconds
  useEffect(() => {
    if (errorMessage) {
      setTimeout(() => setErrorMessage(''), 10000);
    }
  }, [errorMessage]);


  return (
    <div className="container">
      <h1>Welcome to Tauri!</h1>

      {/* Display error message in red */}
      {errorMessage && <p style={{ color: "red" }}>Error: {errorMessage}</p>}
      
      {/* Form that use get_image_base64_from_timestamp to update the imageUrl */}
      <form
        onSubmit={async (e) => {
          e.preventDefault();
          const timestamp =parseInt(e.currentTarget.timestamp.value);
          try {
            const imageBase64 = await invoke("get_image_base64_from_timestamp", { timestamp }) as string;
            setImageUrl(imageBase64);          
          } catch (error) {
            setErrorMessage(error as string);
          }
        }}
      >
        <input type="text" name="timestamp" placeholder="Enter timestamp" />
        <button type="submit">Get Image</button>
      </form>
      
      {/* Display the image if imageUrl is not empty */}
      { imageUrl
        ? imageUrl !== "asset://localhost/"
          ?  <img src={`data:image/jpeg;base64,${imageUrl}`} alt="Select an image with a timestamp" />
          : <p>Image not found</p>
        : <p>Enter a timestamp to view the image</p>
      }
      
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

      {/* Button that allow the user to delete their credentials */}
      <button
        onClick={async () => {
          await invoke("delete_credentials");
        }}
      >
        Delete credentials
      </button>

      <br />

      {/* Toggle button to enabled/disable encryption */}
      <button
        onClick={async () => {
          var enabled = encryptionEnabled;
          try {
            enabled = await invoke("toggle_encryption", { enable: !encryptionEnabled }) as boolean;
          } catch (error) {
            setErrorMessage(error as string);
          } finally {
            setEncryptionEnabled(enabled);
          }
          
          
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
    
    </div>
  );
}
