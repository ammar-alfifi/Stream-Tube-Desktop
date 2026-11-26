const fs = require('fs');
const vm = require('vm');
const path = '/home/ammar/projects/Stream-Tube-Desktop/third_party/ejs';
const context = vm.createContext({console});
vm.runInContext(fs.readFileSync(path + '/yt.solver.lib.min.js', 'utf8'), context);
context.meriyah = context.lib.meriyah;
context.astring = context.lib.astring;
vm.runInContext(fs.readFileSync(path + '/yt.solver.core.min.js', 'utf8'), context);
const challenge = JSON.parse(fs.readFileSync('/home/ammar/.hermes/cache/scratch/ejs-challenge.json', 'utf8'));
const input = {
  type: 'player',
  player: fs.readFileSync('/home/ammar/.hermes/cache/scratch/youtube-player.js', 'utf8'),
  output_preprocessed: false,
  requests: [
    {type: 'sig', challenges: [challenge.s]},
    {type: 'n', challenges: [new URL(challenge.url).searchParams.get('n')]},
  ],
};
console.log(JSON.stringify(context.jsc(input)));
