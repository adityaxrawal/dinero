const fs = require('fs');
const sharp = require('sharp');

const svgBuffer = fs.readFileSync('src-tauri/icons/tray.svg');

sharp(svgBuffer)
  .resize(48, 48)
  .png()
  .toFile('src-tauri/icons/tray@2x.png')
  .then(() => console.log('Successfully generated tray@2x.png'))
  .catch(err => console.error(err));
