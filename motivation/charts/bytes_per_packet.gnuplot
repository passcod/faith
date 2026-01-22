set terminal png size 1200,800 font "sans,10" enhanced
set output 'charts/bytes_per_packet.png'
set title 'Network Efficiency: Average Bytes per Packet (higher is better)'
set xlabel 'Number of Requests'
set ylabel 'Bytes per Packet'
set grid ytics
set key outside right top
set style data histograms
set style histogram clustered gap 1
set style fill solid 0.8 border -1
set boxwidth 0.9
set offset 0,0,graph 0.15,0
set xtics ("1" 0, "10" 1, "100" 2)


plot 'charts/bytes_per_packet_data.txt' using 2:xtic(1) title 'native', \
     '' using 3 title 'node-fetch', \
     '' using 4 title 'Fáith-TCP', \
     '' using 5 title 'Fáith-QUIC', \
     '' using ($0-0.33):2:(sprintf("%.1f",$2)) with labels center offset 0,1 font ",8" notitle, \
     '' using ($0-0.11):3:(sprintf("%.1f",$3)) with labels center offset 0,1 font ",8" notitle, \
     '' using ($0+0.11):4:(sprintf("%.1f",$4)) with labels center offset 0,1 font ",8" notitle, \
     '' using ($0+0.33):5:(sprintf("%.1f",$5)) with labels center offset 0,1 font ",8" notitle
