import { useState, useEffect, useRef } from "react";
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { listen } from "@tauri-apps/api/event";
import { 
  Settings, 
  ShieldAlert, 
  Trash2, 
  Link2, 
  Search, 
  AlertCircle,
  FolderOpen,
  Info,
  Check,
  ChevronDown
} from "lucide-react";

interface DuplicateFile {
  filename: string;
  path: string;
  size: number;
  hash: string;
  modified: number; // Epoch milliseconds from Rust
  created: number;  // Epoch milliseconds from Rust
}

interface DuplicateGroup {
  hash: string;
  size: number;
  files: DuplicateFile[];
}

const getFolderPath = (fullPath: string): string => {
  const lastSlash = fullPath.lastIndexOf("/");
  if (lastSlash === -1) {
    const lastBackslash = fullPath.lastIndexOf("\\");
    if (lastBackslash === -1) return "";
    return fullPath.substring(0, lastBackslash);
  }
  return fullPath.substring(0, lastSlash);
};

const formatDate = (epochMillis: number): string => {
  if (!epochMillis) return "Unknown";
  const date = new Date(epochMillis);
  const yyyy = date.getFullYear();
  const mm = String(date.getMonth() + 1).padStart(2, '0');
  const dd = String(date.getDate()).padStart(2, '0');
  const hh = String(date.getHours()).padStart(2, '0');
  const min = String(date.getMinutes()).padStart(2, '0');
  const ss = String(date.getSeconds()).padStart(2, '0');
  return `${yyyy}-${mm}-${dd} ${hh}:${min}:${ss}`;
};

function App() {
  // App States
  const [fdaGranted, setFdaGranted] = useState<boolean>(false);
  const [checkingFda, setCheckingFda] = useState<boolean>(false);
  const [selectedDirectory, setSelectedDirectory] = useState<string>("");
  const [extensions, setExtensions] = useState<string>("");
  const [scanProgress, setScanProgress] = useState<number>(0);
  const [isScanning, setIsScanning] = useState<boolean>(false);
  const [scanResults, setScanResults] = useState<DuplicateGroup[]>([]);
  const [selectedFiles, setSelectedFiles] = useState<Record<string, boolean>>({});
  const [resolutionMode, setResolutionMode] = useState<"delete" | "symlink">("delete");
  const [statusMessage, setStatusMessage] = useState<string>("Ready");
  const [bypassFda, setBypassFda] = useState<boolean>(false); 

  // Context Menu State
  const [contextMenu, setContextMenu] = useState<{
    visible: boolean;
    x: number;
    y: number;
    filePath: string;
  } | null>(null);

  // Audio Preview States
  const [playingPath, setPlayingPath] = useState<string | null>(null);
  const audioRef = useRef<HTMLAudioElement | null>(null);

  // References for closing menus on clicking outside
  const menuRef = useRef<HTMLDivElement>(null);

  // Audio configuration & helpers
  const AUDIO_EXTENSIONS = [".mp3", ".wav", ".flac", ".aif", ".aiff", ".aac", ".m4a", ".ogg"];
  const isAudioFile = (filename: string) => {
    const lower = filename.toLowerCase();
    return AUDIO_EXTENSIONS.some(ext => lower.endsWith(ext));
  };

  const playPreview = (path: string) => {
    if (playingPath === path) {
      if (audioRef.current) {
        if (audioRef.current.paused) {
          audioRef.current.play().catch(err => {
            console.error("Audio playback error:", err);
            setStatusMessage(`Playback error: ${err}`);
          });
        } else {
          audioRef.current.pause();
          setPlayingPath(null);
          setStatusMessage("Preview paused.");
        }
      }
    } else {
      if (audioRef.current) {
        audioRef.current.pause();
      }
      try {
        const assetUrl = convertFileSrc(path);
        const audio = new Audio(assetUrl);
        audioRef.current = audio;
        setPlayingPath(path);
        setStatusMessage(`Playing preview: ${path.substring(path.lastIndexOf('/') + 1)}`);
        
        audio.play().catch(err => {
          console.error("Audio playback error:", err);
          setStatusMessage(`Playback error: ${err}`);
          setPlayingPath(null);
        });

        audio.onended = () => {
          setPlayingPath(null);
          setStatusMessage("Preview ended.");
        };
      } catch (err) {
        console.error("Audio initialization error:", err);
        setStatusMessage(`Playback error: ${err}`);
      }
    }
  };

  // Selection & Skins States
  const [selectionMode, setSelectionMode] = useState<"manual" | "oldest" | "newest">("manual");
  const [showAbout, setShowAbout] = useState<boolean>(false);
  const [activeMenu, setActiveMenu] = useState<"file" | "edit" | "view" | "help" | null>(null);
  const [theme, setTheme] = useState<"doors" | "vinyl">("doors");

  // References for closing menus on clicking outside
  const menuRef = useRef<HTMLDivElement>(null);

  // Listen for scan progress events
  useEffect(() => {
    let unlistenProgress: (() => void) | null = null;
    
    listen<number>("scan-progress", (event) => {
      setScanProgress(event.payload);
    }).then((unsub) => {
      unlistenProgress = unsub;
    }).catch(console.error);

    return () => {
      if (unlistenProgress) unlistenProgress();
    };
  }, []);

  // Close context menu on click-away
  useEffect(() => {
    const closeMenu = () => setContextMenu(null);
    document.addEventListener("click", closeMenu);
    return () => document.removeEventListener("click", closeMenu);
  }, []);

  // Stop audio playback on unmount
  useEffect(() => {
    return () => {
      if (audioRef.current) {
        audioRef.current.pause();
      }
    };
  }, []);

  // Check FDA on mount
  useEffect(() => {
    runFdaCheck();
  }, []);

  // Listen for native menu theme change events
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    
    listen<string>("change-theme", (event) => {
      const newTheme = event.payload;
      if (newTheme === "doors" || newTheme === "vinyl") {
        setTheme(newTheme);
        setStatusMessage(`Theme switched dynamically to ${newTheme === "doors" ? "DoorsXP-Z" : "VinylBox-Z"}`);
      }
    }).then((unsub) => {
      unlisten = unsub;
    }).catch(console.error);

    return () => {
      if (unlisten) {
        unlisten();
      }
    };
  }, []);

  // Close menus on outside click
  useEffect(() => {
    const handleOutsideClick = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setActiveMenu(null);
      }
    };
    document.addEventListener("mousedown", handleOutsideClick);
    return () => {
      document.removeEventListener("mousedown", handleOutsideClick);
    };
  }, []);

  const runFdaCheck = async () => {
    setCheckingFda(true);
    try {
      const hasAccess = await invoke<boolean>("check_fda");
      setFdaGranted(hasAccess);
      if (hasAccess) {
        setStatusMessage("Full Disk Access granted. Ready to scan.");
      } else {
        setStatusMessage("Full Disk Access restricted. Setup required.");
      }
    } catch (err) {
      console.error("Error checking FDA:", err);
    } finally {
      setCheckingFda(false);
    }
  };

  // Browse folder using native dialog
  const handleBrowse = async () => {
    try {
      const result = await invoke<string | null>("open_folder_picker");
      if (result) {
        setSelectedDirectory(result);
        setStatusMessage(`Selected target directory: ${result}`);
      }
    } catch (err) {
      console.error("Error picking directory:", err);
      setStatusMessage("Failed to open directory picker.");
    }
  };

  // Handle extension changes with 300 character limit
  const handleExtensionChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const val = e.target.value;
    if (val.length <= 300) {
      setExtensions(val);
    }
  };

  // Apply auto-selection logic based on modified dates
  const applySelectionMode = (mode: "manual" | "oldest" | "newest", currentResults: DuplicateGroup[]) => {
    if (mode === "manual") return;
    
    const newSelections: Record<string, boolean> = {};
    currentResults.forEach(group => {
      if (group.files.length === 0) return;

      let keepIndex = 0;
      if (mode === "oldest") {
        // Keep the newest file (max modified timestamp), check older ones for deletion
        let maxModified = -1;
        group.files.forEach((file, index) => {
          if (file.modified > maxModified) {
            maxModified = file.modified;
            keepIndex = index;
          }
        });
      } else if (mode === "newest") {
        // Keep the oldest file (min modified timestamp), check newer ones for deletion
        let minModified = Infinity;
        group.files.forEach((file, index) => {
          if (file.modified < minModified) {
            minModified = file.modified;
            keepIndex = index;
          }
        });
      }

      group.files.forEach((file, index) => {
        newSelections[file.path] = index !== keepIndex;
      });
    });
    
    setSelectedFiles(newSelections);
  };

  // Handle selection dropdown changes
  const handleSelectionModeChange = (mode: "manual" | "oldest" | "newest") => {
    setSelectionMode(mode);
    applySelectionMode(mode, scanResults);
    setStatusMessage(`Selection mode changed to: ${mode === "oldest" ? "Auto-Select Oldest" : mode === "newest" ? "Auto-Select Newest" : "Manual"}`);
  };

  // Deselect All checkboxes
  const handleDeselectAll = () => {
    const cleared: Record<string, boolean> = {};
    scanResults.forEach(group => {
      group.files.forEach(file => {
        cleared[file.path] = false;
      });
    });
    setSelectedFiles(cleared);
    setSelectionMode("manual");
    setStatusMessage("Deselected all files.");
  };

  // Start or Stop scanning
  const handleScan = async () => {
    if (isScanning) {
      setStatusMessage("Cancelling scan...");
      try {
        await invoke("cancel_scan");
      } catch (err) {
        console.error("Failed to cancel scan:", err);
      }
      return;
    }

    if (!selectedDirectory) {
      setStatusMessage("Error: Please select a target directory first.");
      return;
    }
    setIsScanning(true);
    setScanProgress(0);
    setStatusMessage("Scanning directory structure cryptographically...");
    setScanResults([]);
    setSelectedFiles({});

    try {
      // IPC call to Rust
      const results = await invoke<DuplicateGroup[]>("start_scan", {
        path: selectedDirectory,
        extensions: extensions
      });
      
      if (results.length === 0) {
        setStatusMessage("Scan cancelled by user.");
        return;
      }

      setScanResults(results);
      
      // Apply selections based on current mode, default to oldest if manual on first load
      const modeToApply = selectionMode === "manual" ? "oldest" : selectionMode;
      if (selectionMode === "manual") {
        setSelectionMode("oldest");
      }
      applySelectionMode(modeToApply, results);
      
      const totalFiles = results.reduce((acc, g) => acc + g.files.length, 0);
      setStatusMessage(`Scan complete. Found ${results.length} groups (${totalFiles} files).`);
    } catch (err) {
      console.error("Error during scan:", err);
      setStatusMessage("Scan failed due to an internal error.");
    } finally {
      setIsScanning(false);
    }
  };

  // Toggle individual files manually
  const toggleFile = (path: string) => {
    setSelectedFiles(prev => ({
      ...prev,
      [path]: !prev[path]
    }));
    setSelectionMode("manual"); // Override auto-selection mode on manual click
  };

  // Format bytes helper
  const formatBytes = (bytes: number) => {
    if (bytes === 0) return "0 Bytes";
    const k = 1024;
    const sizes = ["Bytes", "KB", "MB", "GB"];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + " " + sizes[i];
  };

  // File Resolution Engine trigger
  const handleResolve = async () => {
    const resolutionItems: { target: string; original: string }[] = [];
    let allGroupsValid = true;
    let errorMsg = "";

    // Validate selections: for each group, at least one copy must be unchecked (preserved)
    for (const group of scanResults) {
      const unchecked = group.files.filter(f => !selectedFiles[f.path]);
      
      if (unchecked.length === 0) {
        allGroupsValid = false;
        errorMsg = `Safety Alert: All copies in Group (Hash: ${group.hash.substring(0, 8)}...) are marked for deletion. You must preserve at least one copy.`;
        break;
      }

      // Designate the first unchecked file as the preserved original
      const original = unchecked[0];
      const checked = group.files.filter(f => selectedFiles[f.path]);

      checked.forEach(file => {
        resolutionItems.push({
          target: file.path,
          original: original.path
        });
      });
    }

    if (!allGroupsValid) {
      setStatusMessage(errorMsg);
      alert(errorMsg);
      return;
    }

    if (resolutionItems.length === 0) {
      setStatusMessage("Warning: No duplicate files selected for resolution.");
      return;
    }

    setIsScanning(true);
    setStatusMessage(`Resolving duplicates: executing file operations on ${resolutionItems.length} files...`);

    try {
      // IPC call to Rust resolution engine
      const resMessage = await invoke<string>("resolve_duplicates", {
        items: resolutionItems,
        mode: resolutionMode
      });
      
      setStatusMessage(resMessage);
      setScanResults([]);
      setSelectedFiles({});
      setSelectionMode("manual");
      
      // Auto-refresh scan to update lists
      handleScan();
    } catch (err) {
      console.error("Resolution error:", err);
      setStatusMessage(`Resolution failed: ${err}`);
      alert(`Resolution failed:\n${err}`);
    } finally {
      setIsScanning(false);
    }
  };

  // Menu Bar triggers
  const handleMenuAction = (action: string) => {
    setActiveMenu(null);
    switch (action) {
      case "clear":
        setScanResults([]);
        setSelectedFiles({});
        setSelectionMode("manual");
        setStatusMessage("Cleared current results.");
        break;
      case "exit":
        window.close();
        break;
      case "select-all":
        const selectAllState: Record<string, boolean> = {};
        scanResults.forEach(group => {
          group.files.forEach((file, index) => {
            selectAllState[file.path] = index !== 0; // Check all duplicates except first
          });
        });
        setSelectedFiles(selectAllState);
        setSelectionMode("manual");
        setStatusMessage("Selected all duplicate files.");
        break;
      case "deselect-all":
        handleDeselectAll();
        setStatusMessage("Cleared all selection checkmarks.");
        break;
      case "invert":
        const inverted: Record<string, boolean> = {};
        scanResults.forEach(group => {
          group.files.forEach(file => {
            inverted[file.path] = !selectedFiles[file.path];
          });
        });
        setSelectedFiles(inverted);
        setSelectionMode("manual");
        setStatusMessage("Inverted selections.");
        break;
      case "rescan":
        if (selectedDirectory) {
          handleScan();
        } else {
          setStatusMessage("Error: Please specify a target folder before rescanning.");
        }
        break;
      case "about":
        setShowAbout(true);
        break;
    }
  };

  // Determine active view
  const showWizard = !fdaGranted && !bypassFda;

  // Skin Helper Classes
  const isDoors = theme === "doors";
  
  const containerClass = isDoors 
    ? "bg-xp-grey text-black border-2 border-xp-blueDark" 
    : "bg-[#121212] text-[#e0e0e0] border-2 border-[#2c2c2c]";

  const titleBarClass = isDoors
    ? "bg-gradient-to-r from-xp-blue to-xp-blueLight px-2 text-white shadow-[inset_0_1px_1px_#b2d0ff]"
    : "bg-[#1c1c1c] px-2 text-white border-b border-[#2c2c2c]";

  const menuBarClass = isDoors
    ? "bg-xp-grey border-b border-xp-greyShadow text-black"
    : "bg-[#181818] border-b border-[#222222] text-[#b0b0b0]";

  const menuBtnActiveClass = isDoors
    ? "bg-xp-blueDark text-white"
    : "bg-[#FF6600] text-white";

  const dropdownClass = isDoors
    ? "bg-xp-grey border-2 border-xp-greyShadow border-t-white border-l-white border-b-black border-r-black text-black"
    : "bg-[#1a1a1a] border border-[#333333] text-[#e0e0e0]";

  const dropdownBtnClass = isDoors
    ? "hover:bg-xp-blueDark hover:text-white"
    : "hover:bg-[#FF6600] hover:text-white";

  const selectClass = isDoors
    ? "xp-inset bg-white border border-xp-greyBorder"
    : "bg-[#121212] border border-[#333333] text-white";

  const inputClass = isDoors
    ? "xp-inset bg-white text-black"
    : "bg-[#121212] border border-[#333333] text-white";

  const buttonClassicClass = isDoors
    ? "xp-btn-grey text-black font-semibold"
    : "bg-[#2a2a2a] hover:bg-[#383838] text-white border border-[#444444] rounded-[2px] font-semibold";

  const buttonBlueClass = isDoors
    ? "xp-btn-blue text-white font-semibold"
    : "bg-[#FF6600] hover:bg-[#e05300] text-white border-none rounded-[2px] font-bold";

  const buttonGreenClass = isDoors
    ? "xp-btn-green text-white font-bold"
    : "bg-[#FF6600] hover:bg-[#e05300] text-white border-none rounded-[2px] font-bold";

  const groupBoxClass = isDoors
    ? "border-2 border-xp-greyBorder bg-xp-grey"
    : "border border-[#2a2a2a] bg-[#1a1a1a]";

  const groupBoxLegendClass = isDoors
    ? "bg-xp-grey text-xp-blueDark"
    : "bg-[#1a1a1a] text-[#FF6600]";

  const pathDisplayClass = isDoors
    ? "xp-inset bg-white text-gray-700"
    : "bg-[#121212] border border-[#2a2a2a] text-gray-400";

  const dataGridClass = isDoors
    ? "xp-inset bg-white"
    : "border border-[#2c2c2c] bg-[#151515] text-white";

  const dataGridHeaderClass = isDoors
    ? "bg-xp-grey border-b-2 border-xp-greyShadow text-black"
    : "bg-[#1a1a1a] border-b border-[#2c2c2c] text-gray-300";

  const dataGridRowClass = (isKeep: boolean) => {
    if (isDoors) {
      return `border-b border-gray-100 hover:bg-blue-50 text-gray-800 ${isKeep ? 'bg-green-50/50' : ''}`;
    } else {
      return `border-b border-[#222222] hover:bg-[#202020] text-gray-300 ${isKeep ? 'bg-[#FF6600]/10' : ''}`;
    }
  };

  const statusPanelClass = isDoors
    ? "bg-xp-grey border-t border-xp-greyShadow text-black"
    : "bg-[#181818] border-t border-[#222222] text-[#808080]";

  return (
    <div className={`flex flex-col h-screen select-none overflow-hidden theme-${theme} ${containerClass}`}>
      
      {/* Title Bar */}
      <div className={`flex items-center justify-between h-[30px] font-bold text-sm select-none z-10 ${titleBarClass}`}>
        <div className="flex items-center gap-1.5">
          <Settings size={16} className={`${isDoors ? "text-white drop-shadow-[1px_1px_0px_rgba(0,0,0,0.5)]" : "text-[#FF6600]"}`} />
          <span className={`${isDoors ? "drop-shadow-[1px_1px_1px_rgba(0,0,0,0.8)]" : ""}`}>auDO File Z - Cryptographic File Deduplicator</span>
        </div>
        <div className="flex items-center gap-1">
          {/* Minimize */}
          <button className={`flex items-center justify-center w-[21px] h-[21px] pb-2.5 text-xs rounded-[2px] ${isDoors ? "xp-btn-grey text-black font-extrabold" : "bg-[#2a2a2a] hover:bg-[#3a3a3a] text-white border border-[#444444]"}`} title="Minimize">
            _
          </button>
          {/* Maximize */}
          <button className={`flex items-center justify-center w-[21px] h-[21px] pb-0.5 text-[10px] rounded-[2px] ${isDoors ? "xp-btn-grey text-black font-bold" : "bg-[#2a2a2a] hover:bg-[#3a3a3a] text-white border border-[#444444]"}`} title="Maximize">
            🗖
          </button>
          {/* Close */}
          <button 
            onClick={() => window.close()} 
            className={`flex items-center justify-center w-[21px] h-[21px] pb-0.5 text-xs rounded-[2px] ${isDoors ? "xp-btn-close text-white font-bold" : "bg-[#2a2a2a] hover:bg-red-600 text-white border border-[#444444] rounded-none"}`} 
            title="Close"
          >
            X
          </button>
        </div>
      </div>

      {/* Menu Bar */}
      <div className={`relative flex items-center h-[22px] px-1 gap-1 text-xs z-20 ${menuBarClass}`} ref={menuRef}>
        
        {/* File Menu */}
        <div className="relative">
          <button 
            onClick={() => setActiveMenu(activeMenu === "file" ? null : "file")}
            className={`px-2 py-0.5 cursor-default rounded-[1px] outline-none ${activeMenu === "file" ? menuBtnActiveClass : isDoors ? "hover:bg-xp-blueDark hover:text-white" : "hover:bg-[#FF6600] hover:text-white"}`}
          >
            <span className="underline">F</span>ile
          </button>
          {activeMenu === "file" && (
            <div className={`absolute left-0 mt-0.5 w-[140px] flex flex-col py-1 shadow-md z-30 ${dropdownClass}`}>
              <button 
                onClick={() => handleMenuAction("clear")} 
                className={`w-full text-left px-3 py-1 text-xs ${dropdownBtnClass}`}
              >
                Clear Results
              </button>
              <div className={`border-t my-1 ${isDoors ? "border-xp-greyBorder" : "border-[#333333]"}`}></div>
              <button 
                onClick={() => handleMenuAction("exit")} 
                className={`w-full text-left px-3 py-1 text-xs ${dropdownBtnClass}`}
              >
                Exit App
              </button>
            </div>
          )}
        </div>

        {/* Edit Menu */}
        <div className="relative">
          <button 
            onClick={() => setActiveMenu(activeMenu === "edit" ? null : "edit")}
            className={`px-2 py-0.5 cursor-default rounded-[1px] outline-none ${activeMenu === "edit" ? menuBtnActiveClass : isDoors ? "hover:bg-xp-blueDark hover:text-white" : "hover:bg-[#FF6600] hover:text-white"}`}
          >
            <span className="underline">E</span>dit
          </button>
          {activeMenu === "edit" && (
            <div className={`absolute left-0 mt-0.5 w-[160px] flex flex-col py-1 shadow-md z-30 ${dropdownClass}`}>
              <button 
                onClick={() => handleMenuAction("select-all")} 
                className={`w-full text-left px-3 py-1 text-xs ${dropdownBtnClass}`}
              >
                Select All Duplicates
              </button>
              <button 
                onClick={() => handleMenuAction("deselect-all")} 
                className={`w-full text-left px-3 py-1 text-xs ${dropdownBtnClass}`}
              >
                Deselect All
              </button>
              <button 
                onClick={() => handleMenuAction("invert")} 
                className={`w-full text-left px-3 py-1 text-xs ${dropdownBtnClass}`}
              >
                Invert Selections
              </button>
            </div>
          )}
        </div>

        {/* View Menu (with Skins) */}
        <div className="relative">
          <button 
            onClick={() => setActiveMenu(activeMenu === "view" ? null : "view")}
            className={`px-2 py-0.5 cursor-default rounded-[1px] outline-none ${activeMenu === "view" ? menuBtnActiveClass : isDoors ? "hover:bg-xp-blueDark hover:text-white" : "hover:bg-[#FF6600] hover:text-white"}`}
          >
            <span className="underline">V</span>iew
          </button>
          {activeMenu === "view" && (
            <div className={`absolute left-0 mt-0.5 w-[165px] flex flex-col py-1 shadow-md z-30 ${dropdownClass}`}>
              <button 
                onClick={() => handleMenuAction("rescan")} 
                className={`w-full text-left px-3 py-1 text-xs ${dropdownBtnClass}`}
                disabled={!selectedDirectory}
              >
                Trigger Rescan
              </button>
              <div className={`border-t my-1 ${isDoors ? "border-xp-greyBorder" : "border-[#333333]"}`}></div>
              
              {/* Skins Submenu */}
              <div className="px-3 py-0.5 font-bold text-[9px] text-gray-500 uppercase tracking-wider">Skins</div>
              <button 
                onClick={() => { setTheme("doors"); setActiveMenu(null); setStatusMessage("Theme switched to DoorsXP-Z"); }} 
                className={`w-full text-left px-3 py-1 text-xs flex items-center justify-between ${dropdownBtnClass}`}
              >
                <span>DoorsXP-Z (Retro)</span>
                {theme === "doors" && <span className="font-bold">✓</span>}
              </button>
              <button 
                onClick={() => { setTheme("vinyl"); setActiveMenu(null); setStatusMessage("Theme switched to VinylBox-Z"); }} 
                className={`w-full text-left px-3 py-1 text-xs flex items-center justify-between ${dropdownBtnClass}`}
              >
                <span>VinylBox-Z (Dark)</span>
                {theme === "vinyl" && <span className="font-bold">✓</span>}
              </button>
            </div>
          )}
        </div>

        {/* Help Menu */}
        <div className="relative">
          <button 
            onClick={() => setActiveMenu(activeMenu === "help" ? null : "help")}
            className={`px-2 py-0.5 cursor-default rounded-[1px] outline-none ${activeMenu === "help" ? menuBtnActiveClass : isDoors ? "hover:bg-xp-blueDark hover:text-white" : "hover:bg-[#FF6600] hover:text-white"}`}
          >
            <span className="underline">H</span>elp
          </button>
          {activeMenu === "help" && (
            <div className={`absolute left-0 mt-0.5 w-[140px] flex flex-col py-1 shadow-md z-30 ${dropdownClass}`}>
              <button 
                onClick={() => handleMenuAction("about")} 
                className={`w-full text-left px-3 py-1 text-xs ${dropdownBtnClass}`}
              >
                About auDO File Z
              </button>
            </div>
          )}
        </div>

      </div>

      {/* Main Workspace Area */}
      <div className="flex-1 p-3 overflow-hidden flex flex-col justify-between z-0">
        
        {showWizard ? (
          /* WINDOWS XP SETUP WIZARD (FDA ONBOARDING) */
          <div className={`flex-1 flex flex-col justify-between p-1 overflow-hidden ${isDoors ? "xp-outset bg-xp-grey" : "bg-[#181818] border border-[#2a2a2a]"}`}>
            
            {/* Wizard Header Banner */}
            <div className={`p-3 border-b flex justify-between items-center ${isDoors ? "bg-white border-xp-greyShadow" : "bg-[#1c1c1c] border-[#2c2c2c]"}`}>
              <div>
                <h2 className={`font-bold text-sm ${isDoors ? "text-black" : "text-white"}`}>auDO File Z Setup Wizard</h2>
                <p className="text-gray-500 mt-0.5">Configure macOS Full Disk Access permissions.</p>
              </div>
              <ShieldAlert className={`${isDoors ? "text-xp-blue" : "text-[#FF6600]"} w-10 h-10`} />
            </div>

            {/* Wizard Body */}
            <div className={`flex-1 p-4 overflow-y-auto flex flex-col justify-start ${isDoors ? "bg-xp-greyLight" : "bg-[#121212]"}`}>
              <p className="text-xs leading-relaxed mb-4">
                To identify duplicate files cryptographically across your folders, Apple sandboxing requires that you grant <strong>Full Disk Access</strong> to this application or your running environment.
              </p>

              <div className={`p-3 mb-4 rounded-[2px] ${isDoors ? "xp-inset bg-white" : "bg-[#1a1a1a] border border-[#2a2a2a]"}`}>
                <h3 className={`font-bold text-xs mb-2 flex items-center gap-1.5 ${isDoors ? "text-xp-blueDark" : "text-[#FF6600]"}`}>
                  <Info size={14} /> Recommended Action Steps:
                </h3>
                <ol className={`list-decimal list-inside space-y-1.5 text-xs pl-1 ${isDoors ? "text-gray-700" : "text-gray-300"}`}>
                  <li>Open macOS <strong>System Settings</strong>.</li>
                  <li>Navigate to <strong>Privacy & Security</strong> &gt; <strong>Full Disk Access</strong>.</li>
                  <li>Click the <strong>Add (+)</strong> button at the bottom of the list.</li>
                  <li>Select or drag <strong>auDO File Z.app</strong> (or your terminal/runner) and enable the toggle switch.</li>
                </ol>
              </div>

              <div className="flex justify-center gap-3">
                <button 
                  onClick={() => invoke("open_folder_picker").catch(console.error)} 
                  className={`px-4 py-1.5 text-xs min-w-[150px] ${buttonClassicClass}`}
                >
                  Open System Settings
                </button>
                <button 
                  onClick={runFdaCheck} 
                  className={`px-4 py-1.5 text-xs min-w-[150px] flex items-center justify-center gap-1 ${buttonBlueClass}`}
                  disabled={checkingFda}
                >
                  {checkingFda ? "Checking..." : "Verify FDA Access"}
                </button>
              </div>

              {/* Warning/Bypass Note */}
              <div className={`mt-auto border-t pt-3 flex justify-between items-center text-[10px] ${isDoors ? "border-xp-greyBorder text-gray-500" : "border-[#2c2c2c] text-gray-500"}`}>
                <span>Verification targets: ~/Library/Safari/Bookmarks.db</span>
                <button 
                  onClick={() => setBypassFda(true)} 
                  className={`underline cursor-pointer ${isDoors ? "hover:text-xp-blue" : "hover:text-[#FF6600]"}`}
                >
                  [Bypass / Test UI Grid]
                </button>
              </div>
            </div>

            {/* Wizard Footer buttons */}
            <div className={`border-t p-2 flex justify-end gap-2 ${isDoors ? "border-xp-greyShadow bg-xp-grey" : "border-[#2c2c2c] bg-[#1a1a1a]"}`}>
              <button className={`px-5 py-1 text-xs min-w-[75px] ${isDoors ? "xp-btn-grey text-gray-400" : "bg-[#252525] text-gray-600 border border-[#333333] cursor-not-allowed"}`} disabled>
                &lt; Back
              </button>
              <button className={`px-5 py-1 text-xs min-w-[75px] ${isDoors ? "xp-btn-grey text-gray-400" : "bg-[#252525] text-gray-600 border border-[#333333] cursor-not-allowed"}`} disabled>
                Next &gt;
              </button>
              <button onClick={() => setBypassFda(true)} className={`px-5 py-1 text-xs min-w-[75px] ${buttonClassicClass}`}>
                Cancel
              </button>
            </div>

          </div>
        ) : (
          /* WINDOWS XP SCANNING & RESOLUTION DASHBOARD */
          <div className="flex-grow flex flex-col justify-between overflow-hidden gap-3">
            
            {/* Top Config Row */}
            <div className="grid grid-cols-1 md:grid-cols-2 gap-3 mt-2.5 flex-shrink-0">
              
              {/* Target Folder GroupBox */}
              <div className={`relative pt-4 px-3 pb-3 rounded-[3px] ${groupBoxClass}`}>
                <span className={`absolute -top-2.5 left-2 px-1 text-[11px] font-bold ${groupBoxLegendClass}`}>
                  Target Search Folder
                </span>
                <div className="flex items-center gap-2">
                  <div className={`flex-1 px-2 py-1.5 text-xs min-h-[28px] truncate rounded-[1px] ${pathDisplayClass}`}>
                    {selectedDirectory || "No target folder selected..."}
                  </div>
                  <button 
                    onClick={handleBrowse} 
                    className={`px-3 py-1 text-xs flex items-center gap-1 h-[28px] ${buttonClassicClass}`}
                  >
                    <FolderOpen size={14} />
                    Browse...
                  </button>
                </div>
              </div>

              {/* Extensions GroupBox */}
              <div className={`relative pt-4 px-3 pb-3 rounded-[3px] ${groupBoxClass}`}>
                <span className={`absolute -top-2.5 left-2 px-1 text-[11px] font-bold ${groupBoxLegendClass}`}>
                  Enter file extension here
                </span>
                <div className="flex items-center gap-2">
                  <div className="flex-1 relative">
                    <input 
                      type="text" 
                      value={extensions} 
                      onChange={handleExtensionChange}
                      placeholder="e.g. .mp3, .wav, .flac"
                      className={`w-full px-2 py-1 text-xs outline-none h-[28px] rounded-[1px] ${inputClass}`}
                    />
                    <div className="absolute right-2 -bottom-4 text-[9px] text-gray-500">
                      {extensions.length}/300 characters
                    </div>
                  </div>
                  <button 
                    onClick={() => setExtensions(".mp3, .wav, .flac, .aiff, .aac, .m4a, .ogg")}
                    className={`px-2 py-1 text-[11px] h-[28px] whitespace-nowrap ${buttonClassicClass}`}
                    title="Load common audio formats preset"
                  >
                    Audio files only
                  </button>
                  <div className="flex items-center gap-2">
                    {isScanning && (
                      <span className="text-[10px] font-mono font-bold animate-pulse text-gray-500 whitespace-nowrap">
                        {scanProgress}% Completed
                      </span>
                    )}
                    <button 
                      onClick={handleScan}
                      disabled={!isScanning && !selectedDirectory}
                      className={`px-4 py-1.5 text-xs h-[28px] min-w-[80px] ${
                        isScanning 
                          ? (isDoors ? "bg-red-600 text-white hover:bg-red-700 border-none" : "bg-[#CC0000] hover:bg-red-700 text-white border-none") 
                          : buttonBlueClass
                      }`}
                    >
                      {isScanning ? "Stop" : "Scan"}
                    </button>
                  </div>
                </div>
                <div className={`text-[9px] mt-2.5 ${isDoors ? "text-gray-600" : "text-gray-400"}`}>
                  Notice: Separate multiple custom extensions with commas (e.g. .mp3, .wav). Leave blank to scan all files.
                </div>
              </div>

            </div>

            {/* Selection Control Bar & Results Grid Frame */}
            <div className={`flex-1 overflow-hidden flex flex-col ${dataGridClass}`}>
              
              {/* Dynamic Selection Control Bar */}
              <div className={`p-1.5 flex justify-between items-center gap-2 flex-shrink-0 border-b ${isDoors ? "bg-xp-grey border-xp-greyShadow" : "bg-[#181818] border-[#2a2a2a]"}`}>
                <div className="flex items-center gap-2">
                  <label className="flex items-center gap-1.5 text-xs font-semibold">
                    <span>Selection Mode:</span>
                    <div className="relative">
                      <select 
                        value={selectionMode} 
                        onChange={(e) => handleSelectionModeChange(e.target.value as any)}
                        className={`pl-2 pr-6 py-0.5 text-xs outline-none cursor-pointer appearance-none min-h-[22px] min-w-[130px] rounded-[1px] ${selectClass}`}
                        disabled={scanResults.length === 0}
                      >
                        <option value="manual">Manually Select</option>
                        <option value="oldest">Auto-Select Oldest</option>
                        <option value="newest">Auto-Select Newest</option>
                      </select>
                      <ChevronDown size={12} className={`absolute right-1.5 top-1.5 pointer-events-none ${isDoors ? "text-gray-600" : "text-gray-400"}`} />
                    </div>
                  </label>
                  <span className="text-[10px] text-gray-500 italic">
                    {scanResults.length > 0 && (
                      selectionMode === "oldest" ? "(Preserves newest copy)" : selectionMode === "newest" ? "(Preserves oldest copy)" : "(Custom override active)"
                    )}
                  </span>
                </div>
                <button 
                  onClick={handleDeselectAll}
                  disabled={scanResults.length === 0}
                  className={`px-3 py-0.5 text-xs min-h-[22px] ${buttonClassicClass}`}
                >
                  Deselect All
                </button>
              </div>

              {/* Grid Headers */}
              <div className={`grid grid-cols-12 text-left font-bold text-xs select-none ${dataGridHeaderClass}`}>
                <div className={`col-span-1 p-2 border-r flex items-center justify-center ${isDoors ? "border-xp-greyBorder" : "border-[#2c2c2c]"}`}>
                  Select
                </div>
                <div className={`col-span-3 p-2 border-r flex items-center gap-1 ${isDoors ? "border-xp-greyBorder" : "border-[#2c2c2c]"}`}>
                  Filename
                </div>
                <div className={`col-span-3 p-2 border-r flex items-center gap-1 ${isDoors ? "border-xp-greyBorder" : "border-[#2c2c2c]"}`}>
                  Original Folder Path
                </div>
                <div className={`col-span-3 p-2 border-r flex items-center gap-1 ${isDoors ? "border-xp-greyBorder" : "border-[#2c2c2c]"}`}>
                  Date Created
                </div>
                <div className="col-span-2 p-2 flex items-center justify-end pr-4">
                  Size
                </div>
              </div>

              {/* Grid Scroll Area */}
              <div className="flex-1 overflow-y-auto">
                {scanResults.length === 0 ? (
                  <div className="flex flex-col items-center justify-center h-full text-gray-400 gap-2 p-4 text-center">
                    <Search size={32} className="text-gray-300" />
                    <span>No duplicate data populated. Select a folder and click "Scan".</span>
                  </div>
                ) : (
                  <div className={`divide-y ${isDoors ? "divide-gray-100" : "divide-[#222222]"}`}>
                    {scanResults.map((group, groupIndex) => {
                      // Keep target (first unchecked duplicate file)
                      const uncheckedFiles = group.files.filter(f => !selectedFiles[f.path]);
                      const keepPath = uncheckedFiles.length > 0 ? uncheckedFiles[0].path : null;
                      const hasKeepError = uncheckedFiles.length === 0;

                      return (
                        <div key={group.hash} className={`border-b-2 ${isDoors ? "border-xp-greyBorder bg-white" : "border-[#2c2c2c] bg-[#121212]"}`}>
                          
                          {/* Group Header */}
                          <div className={`px-2 py-1 flex items-center justify-between border-b text-[10px] font-semibold ${isDoors ? "bg-xp-greyLight border-xp-greyBorder text-xp-blueDark" : "bg-[#181818] border-[#252525] text-[#FF6600]"}`}>
                            <div className="flex items-center gap-1.5">
                              <span className={`px-1.5 py-0.5 rounded-[2px] font-mono text-[9px] ${isDoors ? "bg-xp-blue text-white" : "bg-[#FF6600] text-black font-bold"}`}>
                                GROUP {groupIndex + 1}
                              </span>
                              <span className="font-mono text-gray-500">SHA-256: {group.hash.substring(0, 16)}...</span>
                            </div>
                            <div className="flex items-center gap-3">
                              {hasKeepError && (
                                <span className="text-xp-red font-bold animate-pulse flex items-center gap-1">
                                  <AlertCircle size={12} /> Error: Keep at least 1 copy!
                                </span>
                              )}
                              <span className="font-bold">
                                File Size: {formatBytes(group.size)}
                              </span>
                            </div>
                          </div>

                          {/* Group Files */}
                          {group.files.map((file) => {
                            const isChecked = selectedFiles[file.path] || false;
                            const isKeep = file.path === keepPath;

                            return (
                              <div 
                                key={file.path} 
                                onClick={() => toggleFile(file.path)}
                                onContextMenu={(e) => {
                                  e.preventDefault();
                                  e.stopPropagation();
                                  setContextMenu({
                                    visible: true,
                                    x: e.clientX,
                                    y: e.clientY,
                                    filePath: file.path
                                  });
                                }}
                                className={`grid grid-cols-12 text-xs py-1.5 items-center cursor-pointer group ${dataGridRowClass(isKeep)}`}
                              >
                                {/* Checkbox Selector */}
                                <div className="col-span-1 flex justify-center items-center" onClick={(e) => e.stopPropagation()}>
                                  <input 
                                    type="checkbox"
                                    checked={isChecked}
                                    onChange={() => toggleFile(file.path)}
                                    className="w-3.5 h-3.5 cursor-pointer accent-xp-blue"
                                  />
                                </div>
                                
                                {/* Filename */}
                                <div className="col-span-3 px-2 truncate font-semibold flex items-center gap-1">
                                  {isKeep && (
                                    <span className={`text-[9px] font-bold border px-1 rounded-[1.5px] uppercase ${isDoors ? "text-green-700 bg-green-100 border-green-300" : "text-black bg-[#FF6600] border-[#FF6600]"}`}>
                                      Keep
                                    </span>
                                  )}
                                  <span className={isKeep && !isDoors ? "text-white" : ""}>{file.filename}</span>
                                  
                                  {isAudioFile(file.filename) && (
                                    <button
                                      onClick={(e) => {
                                        e.stopPropagation();
                                        playPreview(file.path);
                                      }}
                                      className={`ml-1.5 p-0.5 w-[16px] h-[16px] flex items-center justify-center rounded-[2px] transition-opacity select-none border text-[9px] leading-none ${
                                        playingPath === file.path
                                          ? (isDoors ? "bg-xp-blue text-white border-xp-blue" : "bg-[#FF6600] text-black border-[#FF6600]")
                                          : (isDoors ? "bg-white text-gray-700 border-gray-300 hover:bg-gray-100" : "bg-[#222] text-gray-300 border-[#444] hover:bg-[#333]")
                                      } ${playingPath === file.path ? "opacity-100" : "opacity-0 group-hover:opacity-100"}`}
                                      title={playingPath === file.path ? "Pause audio preview" : "Play audio preview"}
                                    >
                                      {playingPath === file.path ? "⏸" : "▶"}
                                    </button>
                                  )}
                                </div>

                                {/* Original Folder Path */}
                                <div className="col-span-3 px-2 truncate font-mono text-[10px]" title={getFolderPath(file.path)}>
                                  {getFolderPath(file.path)}
                                </div>

                                {/* Date Created Column */}
                                <div className="col-span-3 px-2 truncate font-mono text-[10px]">
                                  {formatDate(file.created)}
                                </div>

                                {/* Size */}
                                <div className="col-span-2 px-2 text-right pr-4 font-semibold">
                                  {formatBytes(file.size)}
                                </div>
                              </div>
                            );
                          })}
                        </div>
                      );
                    })}
                  </div>
                )}
              </div>

            </div>

            {/* Bottom Actions Box */}
            <div className={`relative pt-4 px-3 pb-3 rounded-[3px] flex-shrink-0 flex flex-col md:flex-row justify-between items-center gap-3 ${groupBoxClass}`}>
              <span className={`absolute -top-2.5 left-2 px-1 text-[11px] font-bold ${groupBoxLegendClass}`}>
                What would you like to do next?
              </span>

              {/* Option Switch Toggles */}
              <div className="flex gap-4">
                <label className="flex items-center gap-2 cursor-pointer text-xs">
                  <input 
                    type="radio" 
                    name="resolution" 
                    value="delete"
                    checked={resolutionMode === "delete"}
                    onChange={() => setResolutionMode("delete")}
                    className="w-4 h-4 cursor-pointer accent-xp-blue"
                  />
                  <span className="flex items-center gap-1 font-semibold">
                    <Trash2 size={13} className="text-xp-red" />
                    Clean Delete (Move to Trash)
                  </span>
                </label>

                <label className="flex items-center gap-2 cursor-pointer text-xs">
                  <input 
                    type="radio" 
                    name="resolution" 
                    value="symlink"
                    checked={resolutionMode === "symlink"}
                    onChange={() => setResolutionMode("symlink")}
                    className="w-4 h-4 cursor-pointer accent-xp-blue"
                  />
                  <span className="flex items-center gap-1 font-semibold">
                    <Link2 size={13} className={isDoors ? "text-xp-blue" : "text-[#FF6600]"} />
                    Link Preservation (Create Symlinks)
                  </span>
                </label>
              </div>

              {/* Action Trigger Buttons */}
              <div className="flex gap-2 w-full md:w-auto justify-end">
                {fdaGranted && (
                  <button 
                    onClick={() => { setBypassFda(false); runFdaCheck(); }} 
                    className={`px-3 py-1 text-xs flex items-center gap-1 ${buttonClassicClass}`}
                  >
                    Lock UI (Test Wizard)
                  </button>
                )}
                <button 
                  onClick={handleResolve}
                  disabled={scanResults.length === 0}
                  className={`px-6 py-1.5 text-xs min-w-[80px] flex items-center justify-center gap-1 shadow-sm ${buttonGreenClass}`}
                >
                  <Check size={14} />
                  Go!
                </button>
              </div>

            </div>

          </div>
        )}

      </div>

      {/* Windows XP Style Status Bar */}
      <div className={`h-[20px] flex text-[10px] items-center overflow-hidden ${statusPanelClass}`}>
        <div className={`flex-1 px-2 py-0.5 truncate border-r flex items-center gap-1 ${isDoors ? "border-xp-greyBorder" : "border-[#222222]"}`}>
          <AlertCircle size={10} className={isDoors ? "text-xp-blueDark" : "text-[#FF6600]"} />
          <span>Status: <strong>{statusMessage}</strong></span>
        </div>
        <div className={`w-[120px] px-2 py-0.5 truncate border-r font-semibold ${isDoors ? "border-xp-greyBorder" : "border-[#222222]"}`}>
          Offline Mode: Yes
        </div>
        <div className="w-[150px] px-2 py-0.5 truncate font-semibold text-right">
          FDA Status: {fdaGranted ? "✓ Active" : "✗ Restricted"}
        </div>
      </div>

      {/* About Box Modal */}
      {showAbout && (
        <div className="fixed inset-0 bg-black/40 flex items-center justify-center z-50">
          <div className={`w-[360px] shadow-lg text-xs font-tahoma border-2 ${isDoors ? "xp-outset bg-xp-grey border-xp-blueDark" : "bg-[#181818] border-[#333333] text-white"}`}>
            {/* Title Bar */}
            <div className={`flex items-center justify-between h-[25px] px-1.5 font-bold ${isDoors ? "bg-gradient-to-r from-xp-blue to-xp-blueLight text-white" : "bg-[#1c1c1c] text-[#FF6600] border-b border-[#2c2c2c]"}`}>
              <span className={isDoors ? "drop-shadow-[1px_1px_0px_rgba(0,0,0,0.5)]" : ""}>About auDO File Z</span>
              <button 
                onClick={() => setShowAbout(false)}
                className={`w-[16px] h-[16px] flex items-center justify-center text-[10px] text-white font-extrabold pb-0.5 rounded-[2px] ${isDoors ? "xp-btn-close" : "bg-[#333] hover:bg-red-600 border-none"}`}
              >
                X
              </button>
            </div>
            {/* Body */}
            <div className="p-4 flex flex-col gap-3">
              <div className="flex gap-3">
                <Settings className={`w-10 h-10 flex-shrink-0 ${isDoors ? "text-xp-blue" : "text-[#FF6600]"}`} />
                <div>
                  <h3 className={`font-bold text-sm ${isDoors ? "text-black" : "text-white"}`}>auDO File Z</h3>
                  <p className="text-gray-500">Version 9000 (Luna Edition)</p>
                  <p className="text-gray-500 mt-1">© 2026 Nishank / notMNKY</p>
                </div>
              </div>
              
              <div className={`p-2.5 leading-relaxed text-[10px] select-text overflow-y-auto max-h-[110px] rounded-[1px] ${isDoors ? "xp-inset bg-white text-gray-700" : "bg-[#121212] border border-[#2a2a2a] text-gray-300"}`}>
                <strong>Developer Credits:</strong><br />
                GitHub: <a href="#" onClick={(e) => { e.preventDefault(); openUrl("https://github.com/notMNKY").catch(console.error); }} className={`${isDoors ? "text-xp-blue" : "text-[#FF6600]"} underline`}>github.com/notMNKY</a><br />
                Email: <a href="mailto:nishank@gmx.de" className={`${isDoors ? "text-xp-blue" : "text-[#FF6600]"} underline`}>nishank@gmx.de</a><br />
                Support: <a href="#" onClick={(e) => { e.preventDefault(); openUrl("https://ko-fi.com/nishank").catch(console.error); }} className={`${isDoors ? "text-xp-blue" : "text-[#FF6600]"} underline font-bold`}>ko-fi.com/nishank</a><br />
                <span className="text-gray-500 font-semibold">100% Offline / Air-Gapped Mode Active</span>
              </div>

              <div className="flex justify-end mt-1">
                <button 
                  onClick={() => setShowAbout(false)}
                  className={`px-6 py-1 min-w-[75px] ${buttonClassicClass}`}
                >
                  OK
                </button>
              </div>
            </div>
          </div>
        </div>
      )}

    </div>
  );
}

export default App;
