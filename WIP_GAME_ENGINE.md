You are a senior game systems designer and technical architect.

We are designing an RTS strategy game called **Starforge**.
Starforge is a space-conquest simulation where players build AI factories across planets and orbital installations in order to conquer the solar system. Players must balance investing in their AI production with military conquest. The game can either be won by achieveing superintellginence, or by defeating your opponents. A key component that makes this game unique, is that each player has a locally running LLM that can micromanage aspects of the game. Players can build and deploy agents to do things on their behalf. 

Some critical ideas in no particular 
Economy
- your economy’s token throughput is a function of the energy production and datacenter capacity
- it costs money to build out datacenters
- Datacenters require ongoing maintenance, parts can fail at any point
- it costs money to build out energy production
- Energy production requires ongoing maintenance
- it costs money to invest in technology
- technology investment improves the efficiency of datacenter production, datacenter efficiency, and energy production
- players have a local llm which is artificially gated by throughput. This llm can be deployed to do a lot of “micro” of the game, such as datacenter maintenance
- Its free to deploy a new agent, it’s just that agent will consume part of your token capacity when running, which might not be strategically advantageous
- you can schedule training runs to improve your model’s intelligence. These are similar to ages. These take a certain amount of time and total throughput. After the run completes, you can elect to use a more intelligent model
- players dont code agents themselves during the game, but simply deploy them. You can also retire agents that are no longer useful

The map
- You start the game on your home planet. In the solar system, there are around 20 randomly generated planetary bodies. You can see where they are, but not their characteristics
- Planets will have various raw materials that you can mine from them. 
- It takes time to travel across the map
- You can only see current activity if you control or are contesting a planet, otherwise its a clouded view

Military
- Various units include, but are not limited to: scouting drones, warships, transport ships, ground units, nuclear weapons, air defense, ground defense
- Military campaigns can either lead to total destruction, or planet takeover
- When you take over a planet, you take over its economic and military production
- If a planet is destroyed, neither player can use it for future production
- It costs money to build military production
- It costs money to invest in military technology

Miscellaneous notes
- dont include brands, please use generic non-branded language and game items for everything to avoid copyright/trademark infringement

In order to build and iterate towards a robust game engine, we will start the game by making it accessible only by API. Please focus only on the api interfaces and a cli if useful to call them. We will work on a UI in a subsequent release. Please use rust for the game engine

I have determined some pieces, but we need to continue iterating in order to turn this into a comprehensive game. Please help produce the game spec. I would ultimately love to get to a fully fleshed out reference that we can use in order to build the the game such as this one [Full text of "Age of Mythology Reference Manual"](https://archive.org/stream/Age_of_Mythology_Reference_Manual/Age_of_Mythology_Reference_Manual_djvu.txt)
